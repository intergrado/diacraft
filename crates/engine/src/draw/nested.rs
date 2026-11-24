//! What a nested instance holds of its group.
//!
//! The specification counted what the files hold on a shape nested in a group that takes its cells from a master: an instance in a group (`Master`) holds
//! `Width`, `Height`, `PinX` and `PinY` locally, almost all as formulas that refer to the enclosing group's own sheet by its `ID` (14,765, 14,596, 14,777 and
//! 14,728 of 16,020 shapes), and a member of an instance (`MasterShape` alone) holds the same in 6,022 to 6,232 of 141,440. So a grouped instance's size and place are
//! formulas of its group's, which is how the members follow the group when it is resized.
//!
//! **The form.** R50 says what the formulas refer to, by kind and never by text; gives the shape of a formula that scales with a shape:
//! `Sheet.<id>!Width` or `Sheet.<id>!Height` times a constant. So a member's `Width` and `PinX` are `Sheet.<group>!Width*<factor>` and its `Height` and `PinY`
//! are `Sheet.<group>!Height*<factor>`, the factor being the member's number over the group's, which gives the number the cell held, so that the picture does
//! not change. The cell keeps its cached value (the number it held) and its unit, and `IN` when it had none. **Nothing else is invented.**
//!
//! **Where the specification gives no form the value is kept** (the group size rule in `docs/editing-commands.md` lists them):
//! * a one-dimensional instance: its size and pin are formulas of its ends;
//! * `Width` and `Height` of a member that is turned by other than a half turn: its width lies along the group's height, and no count says which the files tie it to;
//! * a cell of the member that holds a formula of its own already (a file's, or one that passes a number on): it is data the file holds;
//! * a cell with no number anywhere, or a group whose extent along the axis is nothing (a vertical line of boxes of no width), which no factor can reach.
//!
//! **What is not written here, on purpose.** No `Geometry` section, no `LocPinX`, no `LocPinY`: the project rule adds an inherited cell only after an evaluation
//! changes its result, and `calc` does that when a member is resized (instances at their master's size hold a local `Geometry` in
//! 11 of 114, resized ones in 14,336 of 14,506, and then only `Inh` cells in 13,918 of 14,347).

use std::f64::consts::PI;
use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, CellEdit, CellName, CellWrite, Document, FormulaSlot, ShapeRef, SheetRef, Value, WriteOrigin, WriteOutcome};

use super::Problem;
use super::arrange::same_angle;
use super::read::Reader;

/// Which cell of the group a tied cell follows: `Width` for the cells along x, `Height` for those along y.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn cell(self) -> &'static str {
        match self {
            Axis::X => "Width",
            Axis::Y => "Height",
        }
    }
}

/// The four cells R50 counts, with the axis each follows and whether it is a size (a size lies along the shape's own axes, so a turned shape's is not tied).
const TIED: [(&str, Axis, bool); 4] = [("Width", Axis::X, true), ("Height", Axis::Y, true), ("PinX", Axis::X, false), ("PinY", Axis::Y, false)];

/// A group no longer than this (inches) along an axis has no factor to a cell: dividing by it gives a number that is noise.
const MIN_EXTENT: f64 = 1.0e-9;

/// The largest factor written, and the smallest but zero: a cell that is further from its group's size than this is kept as a value, so that the text of a
/// formula is short whatever a hostile file's numbers are.
const MAX_FACTOR: f64 = 1.0e6;
const MIN_FACTOR: f64 = 1.0e-9;

// What a call costs, for the tests that hold it to a count: one for each member looked at and each cell considered. Not kept outside tests.
#[cfg(test)]
thread_local! {
    pub(crate) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n)));
}

#[cfg(not(test))]
fn note_work(_: usize) {}

/// The text of the factor that takes `extent` to `value`, when there is one: the shortest decimal that gives the same number back (the text of an `f64`
/// is never written with an exponent), `0` for nothing. `None` for a group with no extent, a number that is not finite, or a factor outside the range.
fn factor_text(value: f64, extent: f64) -> Option<String> {
    if !value.is_finite() || !extent.is_finite() || extent.abs() < MIN_EXTENT {
        return None;
    }
    let factor = value / extent;
    if factor == 0.0 {
        return Some("0".to_owned());
    }
    (factor.is_finite() && (MIN_FACTOR..=MAX_FACTOR).contains(&factor.abs())).then(|| format!("{factor}"))
}

/// Whether `source` is a tie to shape `parent` in the form this module writes and R30 gives: `Sheet.<parent>!Width` or `Sheet.<parent>!Height` times a number.
fn is_tie_to(source: &str, parent: u32) -> bool {
    let text = source.trim();
    [Axis::X, Axis::Y]
        .into_iter()
        .any(|axis| text.strip_prefix(&format!("Sheet.{parent}!{}*", axis.cell())).is_some_and(|rest| rest.trim().parse::<f64>().is_ok()))
}

/// The cell as a member of a group is to hold it, or `None` when it is to be kept as it is. `local` is the cell the member holds, `found` the cell by
/// inheritance (its value and unit when the member holds none). `parent` is the group the member came from, when it was in one.
fn tied_cell(local: Option<&Cell>, found: Option<&Cell>, group: u32, axis: Axis, extent: f64, parent: Option<u32>) -> Option<Cell> {
    // A formula the shape holds is data the file holds, unless it is a tie to the group the member has left: it is of the form written here, and the
    // member now follows its new group (it would otherwise follow one it is no longer in).
    if let Some(FormulaSlot::Local(f)) = local.map(|cell| &cell.formula)
        && !parent.is_some_and(|p| is_tie_to(&f.source, p))
    {
        return None;
    }
    let value: Value = local.and_then(|cell| cell.value.clone()).or_else(|| found.and_then(|cell| cell.value.clone()))?;
    let unit = local.and_then(|cell| cell.unit.clone()).or_else(|| found.and_then(|cell| cell.unit.clone())).unwrap_or_else(|| Arc::from("IN"));
    let factor = factor_text(value.number()?, extent)?;
    let mut cell = Cell::with_formula(&format!("Sheet.{group}!{}*{factor}", axis.cell()), Some(value));
    // The formula must have been read: a text the parser refuses is not written.
    if !matches!(&cell.formula, FormulaSlot::Local(f) if f.ast.is_ok()) {
        return None;
    }
    cell.unit = Some(unit);
    // What else the cell held (attributes the model has no field for) stays.
    if let Some(local) = local {
        cell.raw = local.raw.clone();
        cell.extra = local.extra.clone();
    }
    Some(cell)
}

/// Writes `Width`, `Height`, `PinX` and `PinY` of each of `members` (the shapes in the group `group_id`, at `group`, which came from the group `parent` when
/// they were in one) that is an instance of a master, or a member of one, as formulas of the group. → how many cells were written. A
/// guarded cell is kept as it is, and so is any cell R50 gives no form for (the module documentation lists them). The cost is a fixed number of lookups
/// for each member.
///
/// **No cell written here is worked out again**: each keeps the number it held, which is what its formula gives, so nothing that reads it has a new
/// result, and a recalculation of the readers would only meet whatever the file's cached results differ from this evaluator's by (`calc`'s).
pub(crate) fn tie_to_group(doc: &mut Document, group_id: u32, group: &ShapeRef, members: &[ShapeRef], parent: Option<u32>) -> Result<usize, Problem> {
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::new();
    {
        let reader = Reader::new(doc);
        let group_sheet = SheetRef::Shape(group.clone());
        let (width, height) = (reader.number(&group_sheet, "Width"), reader.number(&group_sheet, "Height"));
        for at in members {
            note_work(1);
            let Some(shape) = doc.shape(at) else { continue };
            // An instance (the group of a master of several shapes included, which inherits from no one shape), or a shape that stands for one of a
            // master's: what R50 counts. A shape with no master holds what it holds.
            if shape.master.is_none() && !matches!(reader.resolver().master_step(at), Ok(Some(_))) {
                continue;
            }
            let sheet = SheetRef::Shape(at.clone());
            // A line's size and pin are formulas of its ends.
            if reader.is_one_d(&sheet) {
                continue;
            }
            let angle = reader.number(&sheet, "Angle");
            let aligned = same_angle(angle, 0.0) || same_angle(angle, PI);
            for (name, axis, is_size) in TIED {
                note_work(1);
                if is_size && !aligned {
                    continue;
                }
                let extent = if axis == Axis::X { width } else { height };
                let addr = CellAddr::direct(name);
                let found = reader.resolver().resolve(&sheet, &addr);
                let local = shape.sheet.cells.get(&CellName::new(name));
                let Some(cell) = tied_cell(local, found.as_ref().and_then(|f| f.value.cell), group_id, axis, extent, parent) else { continue };
                writes.push((sheet.clone(), CellWrite { addr, edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing }));
            }
        }
    }
    if writes.is_empty() {
        return Ok(0);
    }
    let outcomes = doc.write_batch(writes)?;
    // A cell the write kept (a guard) was not written.
    Ok(outcomes.into_iter().filter(|outcome| *outcome == WriteOutcome::Written).count())
}

/// A cell of a member that is a tie to its group (`Sheet.<group>!Width*<factor>`): where it is and which of the group's cells it follows.
#[derive(Clone, Debug)]
pub(super) struct Tie {
    at: ShapeRef,
    name: &'static str,
    axis: Axis,
}

/// The cells of `members`, shapes directly in the group `group_id`, that are ties to it: the cells whose own formula is of the form
/// this module writes. A cell with another formula is no tie, and a cell with none is none.
pub(super) fn ties_to(doc: &Document, group_id: u32, members: &[ShapeRef]) -> Vec<Tie> {
    let mut found = Vec::new();
    for at in members {
        note_work(1);
        let Some(shape) = doc.shape(at) else { continue };
        for (name, axis, _) in TIED {
            note_work(1);
            let local = shape.sheet.cells.get(&CellName::new(name));
            if let Some(FormulaSlot::Local(f)) = local.map(|cell| &cell.formula)
                && is_tie_to(&f.source, group_id)
            {
                found.push(Tie { at: at.clone(), name, axis });
            }
        }
    }
    found
}

/// Writes the ties again for a group whose size is now `extent` (`Width`, `Height`): each cell of `ties` keeps the number it holds and is given the factor
/// that takes the group's new size to it (the members stay where they are while the group around them is refitted). A cell that no factor
/// can reach (a group of no extent along its axis, a factor out of range) holds its number as a value of its own: the old formula would give another number
/// once the group's size is worked out. Nothing written here is worked out again, as in [`tie_to_group`]. → how many cells were written.
pub(super) fn retie(doc: &mut Document, group_id: u32, ties: &[Tie], extent: (f64, f64)) -> Result<usize, Problem> {
    let mut writes: Vec<(SheetRef, CellWrite)> = Vec::with_capacity(ties.len());
    {
        let reader = Reader::new(doc);
        for tie in ties {
            note_work(1);
            let Some(shape) = doc.shape(&tie.at) else { continue };
            let sheet = SheetRef::Shape(tie.at.clone());
            let addr = CellAddr::direct(tie.name);
            let found = reader.resolver().resolve(&sheet, &addr);
            let found = found.as_ref().and_then(|f| f.value.cell);
            let local = shape.sheet.cells.get(&CellName::new(tie.name));
            let along = if tie.axis == Axis::X { extent.0 } else { extent.1 };
            let Some(cell) = tied_cell(local, found, group_id, tie.axis, along, Some(group_id)).or_else(|| held_cell(local, found)) else { continue };
            writes.push((sheet, CellWrite { addr, edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing }));
        }
    }
    if writes.is_empty() {
        return Ok(0);
    }
    let outcomes = doc.write_batch(writes)?;
    Ok(outcomes.into_iter().filter(|outcome| *outcome == WriteOutcome::Written).count())
}

/// The cell as a plain value: the number it holds (its own, or the one it is found to have) and its unit, with what else it held. `None` for a cell that holds
/// no value.
fn held_cell(local: Option<&Cell>, found: Option<&Cell>) -> Option<Cell> {
    let value: Value = local.and_then(|cell| cell.value.clone()).or_else(|| found.and_then(|cell| cell.value.clone()))?;
    let unit = local.and_then(|cell| cell.unit.clone()).or_else(|| found.and_then(|cell| cell.unit.clone())).unwrap_or_else(|| Arc::from("IN"));
    let mut cell = Cell::constant(value);
    cell.unit = Some(unit);
    if let Some(local) = local {
        cell.raw = local.raw.clone();
        cell.extra = local.extra.clone();
    }
    Some(cell)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::draw::{drop_shape, group_shapes, ungroup_shapes};
    use crate::stencil::find_master;

    #[test]
    fn a_factor_is_the_number_over_the_extent_in_the_shortest_text_that_gives_it_back() {
        assert_eq!(factor_text(1.5, 6.0).as_deref(), Some("0.25"));
        assert_eq!(factor_text(0.0, 6.0).as_deref(), Some("0"), "a cell at the group's edge: a factor of nothing, not `-0`");
        assert_eq!(factor_text(-0.0, 6.0).as_deref(), Some("0"));
        assert_eq!(factor_text(-1.5, 6.0).as_deref(), Some("-0.25"), "a pin outside the box has a negative factor");
        assert_eq!(factor_text(1.0, 3.0).and_then(|t| t.parse::<f64>().ok()), Some(1.0 / 3.0), "all the digits are kept");
    }

    #[test]
    fn a_factor_that_cannot_be_written_is_none_and_the_text_never_has_an_exponent() {
        for (value, extent) in [
            (1.0, 0.0),
            (1.0, 1.0e-12),
            (1.0, f64::NAN),
            (f64::NAN, 1.0),
            (f64::INFINITY, 1.0),
            (1.0e300, 1.0),
            (1.0e-300, 1.0),
            (1.0, 1.0e300 * 10.0),
            (1.0, f64::MIN_POSITIVE),
            // A factor that is in range over an extent that is noise: a pin and a group both a trillionth of an inch.
            (1.0e-12, 1.0e-12),
            (0.0, 1.0e-12),
            (1.0e-10, 5.0e-10),
        ] {
            assert_eq!(factor_text(value, extent), None, "{value} over {extent}");
        }
        for (value, extent) in [(1.0e-9, 1.0), (123_456.0, 0.5), (0.1, 3.0), (1.0e3, 1.0e-3)] {
            let text = factor_text(value, extent).unwrap();
            assert!(!text.contains(['e', 'E']) && text.len() < 40, "{text}");
        }
    }

    #[test]
    fn a_formula_with_a_negative_factor_reads_and_the_cell_is_built_from_it() {
        let found = Cell::constant(Value::from_text("-1.5")).with_unit("MM");
        let cell = tied_cell(None, Some(&found), 7, Axis::Y, 6.0, None).unwrap();
        assert_eq!(cell.formula.text(), Some("Sheet.7!Height*-0.25"));
        assert_eq!((cell.value.as_ref().map(Value::text), cell.unit.as_deref()), (Some("-1.5"), Some("MM")));
        let bare = Cell::constant(Value::from_text("2"));
        assert_eq!(
            tied_cell(None, Some(&bare), 7, Axis::X, 4.0, None).unwrap().unit.as_deref(),
            Some("IN"),
            "a cell with no unit is a length in inches"
        );
    }

    #[test]
    fn a_cell_that_holds_a_formula_or_no_number_is_kept_as_it_is() {
        let formula = Cell::with_formula("Sheet.3!Width*0.5", Some(Value::from_text("2")));
        assert_eq!(tied_cell(Some(&formula), None, 7, Axis::X, 4.0, None), None);
        assert_eq!(tied_cell(Some(&formula), None, 7, Axis::X, 4.0, Some(9)), None, "a tie to a shape that is not the group the member left");
        let guarded = Cell::with_formula("GUARD(Width)", Some(Value::from_text("2")));
        assert_eq!(tied_cell(Some(&guarded), None, 7, Axis::X, 4.0, None), None);
        assert_eq!(tied_cell(None, None, 7, Axis::X, 4.0, None), None, "a cell found nowhere has no number to keep");
        let text = Cell::constant(Value::from_text("not a number"));
        assert_eq!(tied_cell(Some(&text), None, 7, Axis::X, 4.0, None), None);
        let value = Cell::constant(Value::from_text("2"));
        assert_eq!(tied_cell(Some(&value), None, 7, Axis::X, 0.0, None), None, "a group of no extent");
    }

    #[test]
    fn a_tie_to_the_group_the_member_left_is_replaced_by_a_tie_to_its_new_group_and_nothing_else_is() {
        let tie = Cell::with_formula("Sheet.3!Width*0.5", Some(Value::from_text("2"))).with_unit("IN");
        let moved = tied_cell(Some(&tie), None, 7, Axis::X, 4.0, Some(3)).unwrap();
        assert_eq!((moved.formula.text(), moved.value.as_ref().map(Value::text)), (Some("Sheet.7!Width*0.5"), Some("2")));
        for kept in [
            "Sheet.3!Width",
            "Sheet.3!Width*0.5+1",
            "GUARD(Sheet.3!Width*0.5)",
            "Sheet.30!Width*0.5",
            "Sheet.3!PinX*0.5",
            "Sheet.3!Width*Height",
            "Width*0.5",
        ] {
            let cell = Cell::with_formula(kept, Some(Value::from_text("2")));
            assert_eq!(tied_cell(Some(&cell), None, 7, Axis::X, 4.0, Some(3)), None, "`{kept}` is not the form that is written");
        }
        assert!(
            is_tie_to("Sheet.3!Height*-0.25", 3)
                && is_tie_to(" Sheet.3!Width*0 ", 3)
                && !is_tie_to("Sheet.3!Width*", 3)
                && !is_tie_to("Sheet.3!Width*x", 3)
        );
    }

    #[test]
    fn what_the_cell_held_besides_its_number_stays() {
        let mut held = Cell::constant(Value::from_text("2")).with_unit("MM");
        held.error = Some(Arc::from("#REF!"));
        let cell = tied_cell(Some(&held), None, 7, Axis::X, 4.0, None).unwrap();
        assert_eq!((cell.unit.as_deref(), cell.error), (Some("MM"), None), "the unit stays, the error of a formula that was a number is not carried");
    }

    /// A blank drawing with `n` rectangles in a row, and their `ID`s.
    fn rectangles(n: u32) -> (Document, Vec<u32>) {
        let mut doc = Document::new_blank();
        let rectangle = find_master("rectangle").unwrap();
        let ids = (0..n).map(|k| drop_shape(&mut doc, 1, rectangle, 1.0 + f64::from(k % 40) * 0.2, 1.0 + f64::from(k / 40)).unwrap().id).collect();
        (doc, ids)
    }

    /// The work counted for grouping `n` instances.
    fn work_of_grouping(n: u32) -> usize {
        let (mut doc, ids) = rectangles(n);
        WORK.with(|w| w.set(0));
        let made = group_shapes(&mut doc, 1, &ids, 0).unwrap();
        assert_eq!(made.members, usize::try_from(n).unwrap());
        WORK.with(|w| w.get())
    }

    #[test]
    fn the_work_is_a_fixed_number_of_steps_for_each_member_and_not_more_for_more_of_them() {
        let (small, large) = (work_of_grouping(100), work_of_grouping(400));
        // One step for the member and one for each of its four cells.
        assert_eq!((small, large), (500, 2_000));
    }

    #[test]
    fn a_group_made_ninety_levels_deep_does_the_work_of_its_two_members_at_every_level() {
        let (mut doc, ids) = rectangles(2);
        let rectangle = find_master("rectangle").unwrap();
        let mut inner = group_shapes(&mut doc, 1, &ids, 0).unwrap().id;
        let mut seen = Vec::new();
        for level in 0..60 {
            let id = drop_shape(&mut doc, 1, rectangle, 1.0 + f64::from(level % 9), 7.0).unwrap().id;
            WORK.with(|w| w.set(0));
            inner = group_shapes(&mut doc, 1, &[inner, id], 0).unwrap().id;
            seen.push(WORK.with(|w| w.get()));
        }
        // Two members, of which one is a group that holds no master, and one instance: 2 looked at, 4 cells of the instance.
        assert!(seen.iter().all(|w| *w == 6), "{seen:?}");
    }

    /// The work counted for taking apart a group of `n` instances that is itself a member of a group (`inside`), or on the page.
    fn work_of_ungrouping(n: u32, inside: bool) -> usize {
        let (mut doc, ids) = rectangles(n + 1);
        let (last, rest) = ids.split_last().unwrap();
        let inner = group_shapes(&mut doc, 1, rest, 0).unwrap().id;
        if inside {
            group_shapes(&mut doc, 1, &[inner, *last], 0).unwrap();
        }
        WORK.with(|w| w.set(0));
        let taken = ungroup_shapes(&mut doc, 1, &[inner]).unwrap();
        assert_eq!(taken.members.len(), usize::try_from(n).unwrap());
        WORK.with(|w| w.get())
    }

    #[test]
    fn taking_apart_a_group_in_a_group_ties_its_members_in_a_fixed_number_of_steps_each_and_one_on_the_page_ties_none() {
        let (small, large) = (work_of_ungrouping(100, true), work_of_ungrouping(400, true));
        assert_eq!((small, large), (500, 2_000), "one step for each member and one for each of its four cells");
        assert_eq!(work_of_ungrouping(400, false), 0, "a group on the page leaves its members on the page: nothing is tied");
    }

    /// A blank drawing with two rectangles grouped: the document, the group's `ID`, and the places of the two members.
    fn grouped_pair() -> (Document, u32, Vec<ShapeRef>) {
        let (mut doc, ids) = rectangles(2);
        let group = group_shapes(&mut doc, 1, &ids, 0).unwrap().id;
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), group).unwrap();
        let members = vec![at.child(0), at.child(1)];
        (doc, group, members)
    }

    /// The text of the formula and the number of the cell `name` that the shape at `at` holds itself.
    fn own(doc: &Document, at: &ShapeRef, name: &str) -> (Option<String>, f64) {
        let cell = doc.shape(at).unwrap().sheet.cells.get(&CellName::new(name)).unwrap();
        (cell.formula.text().map(str::to_owned), cell.value.as_ref().and_then(Value::number).unwrap())
    }

    #[test]
    fn the_ties_of_a_group_are_the_cells_of_its_members_whose_formula_is_of_the_size_of_that_group() {
        let (mut doc, group, members) = grouped_pair();
        assert_eq!(ties_to(&doc, group, &members).len(), 8, "four cells of each of two instances");
        assert_eq!(ties_to(&doc, group + 1, &members).len(), 0, "ties to another shape's size are none of this group's");
        // A cell given a formula of its own is no tie; a cell with no formula is none.
        let write = |doc: &mut Document, at: &ShapeRef, name: &str, cell: Cell| {
            let write = CellWrite { addr: CellAddr::direct(name), edit: CellEdit::Cell(cell), origin: WriteOrigin::Drawing };
            doc.write_batch(vec![(SheetRef::Shape(at.clone()), write)]).unwrap();
        };
        write(&mut doc, &members[0], "PinX", Cell::with_formula("1.25", Some(Value::from_text("1.25"))).with_unit("IN"));
        write(&mut doc, &members[1], "Width", Cell::constant(Value::from_text("1.5")).with_unit("IN"));
        let ties = ties_to(&doc, group, &members);
        assert_eq!(ties.len(), 6);
        assert!(ties.iter().all(|t| !(t.at == members[0] && t.name == "PinX") && !(t.at == members[1] && t.name == "Width")));
    }

    #[test]
    fn a_tie_written_again_holds_the_number_it_held_over_the_new_size_of_its_group() {
        let (mut doc, group, members) = grouped_pair();
        let ties = ties_to(&doc, group, &members);
        let was: Vec<(Option<String>, f64)> = members.iter().flat_map(|m| TIED.map(|(name, _, _)| own(&doc, m, name))).collect();
        // The group takes another size, and the members are written again for it.
        let (width, height) = (3.4, 2.25);
        assert_eq!(retie(&mut doc, group, &ties, (width, height)).unwrap(), 8);
        let now: Vec<(Option<String>, f64)> = members.iter().flat_map(|m| TIED.map(|(name, _, _)| own(&doc, m, name))).collect();
        for (((was_formula, was_number), (formula, number)), (name, axis, _)) in was.iter().zip(&now).zip(members.iter().flat_map(|_| TIED)) {
            assert_eq!(number, was_number, "{name} keeps the number it held");
            let extent = if axis == Axis::X { width } else { height };
            let formula = formula.as_deref().unwrap();
            let prefix = format!("Sheet.{group}!{}*", axis.cell());
            let factor: f64 = formula.strip_prefix(prefix.as_str()).unwrap().parse().unwrap();
            assert!((factor * extent - number).abs() < 1e-12, "{name}: {formula}");
            assert_ne!(Some(formula.to_owned()), was_formula.clone(), "{name} has the factor of the new size");
        }
    }

    #[test]
    fn a_tie_that_no_factor_can_reach_is_the_number_it_held_as_a_value_of_its_own() {
        let (mut doc, group, members) = grouped_pair();
        let ties = ties_to(&doc, group, &members);
        let was: Vec<f64> = members.iter().flat_map(|m| TIED.map(|(name, _, _)| own(&doc, m, name).1)).collect();
        // A group of no width: no factor takes it to a number, and the old formula would give another number once the group is worked out.
        assert_eq!(retie(&mut doc, group, &ties, (0.0, 2.25)).unwrap(), 8);
        for (k, m) in members.iter().enumerate() {
            for (j, (name, axis, _)) in TIED.iter().enumerate() {
                let (formula, number) = own(&doc, m, name);
                assert_eq!(number, was[k * 4 + j], "{name}");
                if *axis == Axis::X {
                    assert_eq!(formula, None, "{name} is a value now");
                } else {
                    assert!(formula.is_some(), "{name} is still a tie");
                }
            }
        }
    }

    #[test]
    fn a_cell_held_as_a_value_has_the_number_and_the_unit_and_what_else_it_held() {
        let mut found = Cell::constant(Value::from_text("2")).with_unit("MM");
        found.raw = Default::default();
        let held = held_cell(None, Some(&found)).unwrap();
        assert_eq!((held.formula.text(), held.value.as_ref().map(Value::text), held.unit.as_deref()), (None, Some("2"), Some("MM")));
        let local = Cell::with_formula("Sheet.3!Width*0.5", Some(Value::from_text("4")));
        let held = held_cell(Some(&local), Some(&found)).unwrap();
        assert_eq!(
            (held.formula.text(), held.value.as_ref().map(Value::text), held.unit.as_deref()),
            (None, Some("4"), Some("MM")),
            "its own number, the unit found, no formula"
        );
        assert!(held_cell(None, None).is_none(), "no number, no cell");
        let bare = Cell::constant(Value::from_text("1"));
        assert_eq!(held_cell(Some(&bare), None).unwrap().unit.as_deref(), Some("IN"));
    }

    #[test]
    fn a_cell_held_as_a_value_keeps_the_attributes_and_the_extra_of_the_cell_it_was_and_takes_none_from_the_one_found() {
        let mut local = Cell::with_formula("Sheet.3!Width*0.5", Some(Value::from_text("4")));
        local.raw = diacraft_doc::RawAttrs::from_pairs([("Local", "kept")]);
        local.extra = Some(Arc::new(diacraft_doc::CellExtra { text: Some(Arc::from("a note")), ..Default::default() }));
        let mut found = Cell::constant(Value::from_text("2")).with_unit("MM");
        found.raw = diacraft_doc::RawAttrs::from_pairs([("Found", "not kept")]);
        found.extra = Some(Arc::new(diacraft_doc::CellExtra { text: Some(Arc::from("not kept")), ..Default::default() }));
        let held = held_cell(Some(&local), Some(&found)).unwrap();
        assert_eq!(held.raw, local.raw, "the cell's own attributes");
        assert_eq!(held.extra, local.extra, "the cell's own text");
        // With no cell of its own there is nothing to keep: what the one found holds is not copied down.
        let held = held_cell(None, Some(&found)).unwrap();
        assert_eq!((held.raw, held.extra), (diacraft_doc::RawAttrs::new(), None));
    }
}
