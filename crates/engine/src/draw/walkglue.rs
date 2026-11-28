//! The cells of an end glued to the whole of a shape, as the application writes them.
//!
//! **What an end glued to the whole of a shape holds** (`_WALKGLUE`-R1 and `_XFTRIGGER`-R1 to R2):
//! a `Connect` row with `ToPart` 3, **the formula `_WALKGLUE(<own trigger>,<other trigger>,WalkPreference)` in both of its end cells** (`BegTrigger` first for the begin's cells and
//! `EndTrigger` first for the end's), **and in its trigger cell (`BegTrigger` for the begin, `EndTrigger` for the end) the formula `_XFTRIGGER(Sheet.<target>!EventXFMod)`**, which
//! holds 2 while the end is glued.
//!
//! **Why DiaCraft writes it.** The application rebuilds glue from the formulas of the end cells when it loads a file, and drops, without a word, a `Connect` row whose end cell holds no
//! glue formula (observed in the application on a connector DiaCraft glued to a shape with no connection point, public issue 16: a copy whose end cells held the `_WALKGLUE` formulas and
//! whose trigger cell held an `_XFTRIGGER` that named the target stayed glued, and moved with it; with the end formulas and no trigger nothing stayed glued). Under the owner's standing
//! rule, that behaviour of the application replaces the "`_WALKGLUE` is never written" for the glue to the whole of a shape. Point glue is not changed: it is `PAR` and
//! `PNT` formulas, which a reader can work out.
//!
//! **What DiaCraft does with them** (unchanged): it cannot work out `_WALKGLUE`, so it keeps the formula as it is and writes the end's value by its own rule (`glue.rs` places an end whose cell calc cannot work out, whoever wrote it).
//! A connector made here and a connector read from a file are then the same to everything that
//! moves, copies, groups or deletes. The result of `_WALKGLUE` is a length with no unit (`_WALKGLUE`-R2), and the trigger's is a plain number.

use std::collections::HashMap;

use diacraft_doc::{Cell, CellAddr, FormulaSlot, Page, Shape, Value};

use super::connectors::Which;
use crate::stencil::number_text;

/// The cached value of the trigger of an end that is glued (2 marks a glued end).
const GLUED: &str = "2";

/// The formula of the two end cells of the end `which`: its own trigger first, then the other end's, then `WalkPreference` (`_WALKGLUE`-R1).
pub(crate) fn end_formula(which: Which) -> &'static str {
    match which {
        Which::Begin => "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)",
        Which::End => "_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)",
    }
}

/// The name of the trigger cell of the end `which`.
pub(crate) fn trigger_name(which: Which) -> &'static str {
    match which {
        Which::Begin => "BegTrigger",
        Which::End => "EndTrigger",
    }
}

/// The formula of the trigger of an end glued to shape `target`: a call that names the target's `EventXFMod` by its `Sheet.` prefix (`_XFTRIGGER`-R1).
pub(crate) fn trigger_formula(target: u32) -> String {
    format!("_XFTRIGGER(Sheet.{target}!EventXFMod)")
}

/// An end cell of the end `which` glued to the whole of a shape: the formula, and the place `value` as its result, a length with no unit (`_WALKGLUE`-R2). `value` is the place the
/// engine put the end at; the next change to the shape it is glued to puts it again (`glue.rs`).
pub(crate) fn end_cell(which: Which, value: f64) -> Cell {
    Cell::with_formula(end_formula(which), Some(Value::from_text(&number_text(value))))
}

/// The trigger cell of an end glued to the whole of shape `target`: the formula, and 2, a plain number with no unit (`_XFTRIGGER`-R2).
pub(crate) fn trigger_cell(target: u32) -> Cell {
    Cell::with_formula(&trigger_formula(target), Some(Value::from_text(GLUED)))
}

/// The call a trigger holds when the shape it named is deleted: the application writes `_XFTRIGGER(REF())` and leaves the value as it was (`_XFTRIGGER`-R1 and R2, which count it in
/// files).
const DELETED: &str = "_XFTRIGGER(REF())";

/// The trigger cell `cell` of an end whose shape `gone` is deleted: the call that named the shape names none now (`_XFTRIGGER(REF())`), and the cell is as it was otherwise. `None` when the
/// cell does not hold the call [`trigger_formula`] makes of `gone`: another formula is not the glue's to rewrite.
pub(crate) fn trigger_after_delete(cell: &Cell, gone: u32) -> Option<Cell> {
    if cell.formula.text() != Some(trigger_formula(gone).as_str()) {
        return None;
    }
    let mut out = cell.clone();
    out.formula = Cell::with_formula(DELETED, None).formula;
    Some(out)
}

// ---- the glue a drawing holds as plain values ---------------------------------------------------------------------------------------------

/// The shapes a page's connectors are glued to as a whole, by connector: for each, the target of the begin and of the end (`ToPart` 3), in that order. A connector with no such end is not in
/// the map.
pub(crate) fn whole_ends(page: &Page) -> HashMap<u32, [Option<u32>; 2]> {
    let mut ends: HashMap<u32, [Option<u32>; 2]> = HashMap::new();
    for row in page.connects.iter().filter(|c| c.to_part == Some(super::glue::WHOLE_SHAPE)) {
        let end = match row.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => 0,
            Some("EndX" | "EndY") => 1,
            _ => continue,
        };
        if let Some(slot) = ends.entry(row.from_sheet).or_default().get_mut(end) {
            slot.get_or_insert(row.to_sheet);
        }
    }
    ends
}

/// Whether a cell is a plain place: a local number with no formula, no error, no attribute of its own and no content beside the value.
fn is_plain_place(cell: &Cell) -> bool {
    matches!(cell.formula, FormulaSlot::Absent)
        && cell.error.is_none()
        && cell.raw.is_empty()
        && cell.extra.is_none()
        && cell.value.as_ref().and_then(Value::number).is_some()
}

/// The shape given the glue formulas of the ends that are glued to the whole of a shape (`ends`: the target of its begin and of its end, from [`whole_ends`]) and hold plain places, as
/// the application needs them: `_WALKGLUE` in the end cell that holds none, with its place as the result, and the trigger naming the target where the end has no
/// formula of its own there. A drawing that DiaCraft made before this change and one read from an `.odg` file (gives a whole-shape end plain values) have such ends,
/// and so has a file of another program that wrote none: **the caller says whose shape this is** (a save across to a VSDX type gives them to a connector of DiaCraft's, the route marker, and
/// not to a file's own); a drawing's own formulas, a file's `_WALKGLUE` among them, are left as they are. `None` when the shape needs none.
pub(crate) fn dress_shape(shape: &Shape, ends: &[Option<u32>; 2]) -> Option<Shape> {
    let mut own: Option<Shape> = None;
    for (which, target) in [Which::Begin, Which::End].into_iter().zip(ends) {
        let Some(target) = *target else { continue };
        let mut cells: Vec<(&str, Cell)> = Vec::new();
        for name in [which.x_cell(), which.y_cell()] {
            if let Some(cell) = shape.sheet.cell(name).filter(|c| is_plain_place(c)) {
                cells.push((name, Cell::with_formula(end_formula(which), cell.value.clone())));
            }
        }
        if cells.is_empty() {
            continue;
        }
        if shape.sheet.cell(trigger_name(which)).is_none_or(|c| matches!(c.formula, FormulaSlot::Absent)) {
            cells.push((trigger_name(which), trigger_cell(target)));
        }
        let copy = own.get_or_insert_with(|| shape.clone());
        let mut counted = copy.counted_sheet();
        for (name, cell) in cells {
            // A sheet at the most cells it may hold takes no more: the shape is then left as it is, with its plain ends.
            if counted.set_cell(&CellAddr::direct(name), cell).is_err() {
                return None;
            }
        }
    }
    own
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;

    use diacraft_doc::{CellName, Connect, Connects};

    use super::*;

    fn plain(name: &str, v: f64) -> (CellName, Cell) {
        (CellName::new(name), Cell::constant(Value::from_text(&number_text(v))).with_unit("IN"))
    }

    /// A connector with plain ends at (1, 2) and (3, 4), as an `.odg` file's is read.
    fn connector() -> Shape {
        let mut shape = Shape::new(9);
        let sheet = Arc::make_mut(&mut shape.sheet);
        for (name, v) in [("BeginX", 1.0), ("BeginY", 2.0), ("EndX", 3.0), ("EndY", 4.0)] {
            let (name, cell) = plain(name, v);
            sheet.cells.insert(name, cell);
        }
        shape
    }

    fn formula(shape: &Shape, name: &str) -> Option<String> {
        shape.sheet.cell(name).and_then(|c| c.formula.text()).map(str::to_owned)
    }

    #[test]
    fn plain_ends_glued_to_the_whole_of_a_shape_are_given_the_formulas_and_the_triggers() {
        let dressed = dress_shape(&connector(), &[Some(4), Some(5)]).expect("two plain ends are dressed");
        for name in ["BeginX", "BeginY"] {
            assert_eq!(formula(&dressed, name).as_deref(), Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)"), "{name}");
        }
        for name in ["EndX", "EndY"] {
            assert_eq!(formula(&dressed, name).as_deref(), Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"), "{name}");
        }
        assert_eq!(formula(&dressed, "BegTrigger").as_deref(), Some("_XFTRIGGER(Sheet.4!EventXFMod)"));
        assert_eq!(formula(&dressed, "EndTrigger").as_deref(), Some("_XFTRIGGER(Sheet.5!EventXFMod)"));
        // The place is the result, and the unit is gone (`_WALKGLUE`-R2).
        let begin_y = dressed.sheet.cell("BeginY").unwrap();
        assert_eq!((begin_y.value.as_ref().and_then(Value::number), begin_y.unit.as_deref()), (Some(2.0), None));
    }

    #[test]
    fn an_end_that_is_not_glued_to_the_whole_of_a_shape_is_left_as_it_is() {
        let dressed = dress_shape(&connector(), &[Some(4), None]).expect("the begin is dressed");
        assert!(formula(&dressed, "EndX").is_none() && formula(&dressed, "EndY").is_none() && formula(&dressed, "EndTrigger").is_none());
        assert!(dress_shape(&connector(), &[None, None]).is_none());
    }

    #[test]
    fn ends_that_hold_formulas_already_are_left_as_they_are_and_so_is_a_trigger_that_has_one() {
        let mut shape = connector();
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet
            .cells
            .insert(CellName::new("BeginX"), Cell::with_formula("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)", Some(Value::from_text("1"))));
        sheet
            .cells
            .insert(CellName::new("BeginY"), Cell::with_formula("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)", Some(Value::from_text("2"))));
        sheet.cells.insert(CellName::new("BegTrigger"), Cell::with_formula("_XFTRIGGER(Sheet.8!EventXFMod)", Some(Value::from_text("2"))));
        assert!(dress_shape(&shape, &[Some(4), None]).is_none(), "a file's own glue is kept as it is");
        // The end has plain cells and a trigger of its own: only the cells are dressed.
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("EndTrigger"), Cell::with_formula("_XFTRIGGER(Sheet.8!EventXFMod)", Some(Value::from_text("2"))));
        let dressed = dress_shape(&shape, &[None, Some(5)]).expect("the end is dressed");
        assert_eq!(formula(&dressed, "EndTrigger").as_deref(), Some("_XFTRIGGER(Sheet.8!EventXFMod)"), "a trigger that has a formula keeps it");
        assert_eq!(formula(&dressed, "EndY").as_deref(), Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"));
    }

    #[test]
    fn a_cell_that_is_no_plain_place_is_not_dressed() {
        let mut shape = connector();
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(CellName::new("BeginX"), Cell::constant(Value::from_text("abc")));
        let mut with_error = Cell::constant(Value::from_text("2"));
        with_error.error = Some(Arc::from("#REF!"));
        sheet.cells.insert(CellName::new("BeginY"), with_error);
        assert!(dress_shape(&shape, &[Some(4), None]).is_none(), "a text and an error are not places");
    }

    #[test]
    fn the_targets_of_the_whole_shape_ends_of_a_page_are_found_by_connector_and_end() {
        let row = |from: u32, cell: &str, to: u32, part: u32| Connect {
            from_sheet: from,
            from_cell: Some(Arc::from(cell)),
            from_part: Some(9),
            to_sheet: to,
            to_cell: Some(Arc::from("PinX")),
            to_part: Some(part),
            ..Connect::default()
        };
        let mut page = Page::new(1, "Page-1");
        page.connects = Connects::from_vec(vec![
            row(7, "BeginX", 1, 3),
            row(7, "EndX", 2, 100),
            row(8, "EndX", 2, 3),
            row(8, "BeginY", 6, 3),
            row(9, "Elsewhere", 1, 3),
        ]);
        let ends = whole_ends(&page);
        assert_eq!(ends.get(&7), Some(&[Some(1), None]), "a point end is no whole-shape end");
        assert_eq!(ends.get(&8), Some(&[Some(6), Some(2)]));
        assert!(!ends.contains_key(&9));
    }

    #[test]
    fn the_end_cells_name_their_own_trigger_first_and_the_other_trigger_second() {
        assert_eq!(end_formula(Which::Begin), "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)");
        assert_eq!(end_formula(Which::End), "_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)");
        assert_eq!((trigger_name(Which::Begin), trigger_name(Which::End)), ("BegTrigger", "EndTrigger"));
    }

    #[test]
    fn the_trigger_names_the_target_by_its_sheet_prefix_and_holds_two_with_no_unit() {
        let cell = trigger_cell(12);
        assert_eq!(cell.formula.text(), Some("_XFTRIGGER(Sheet.12!EventXFMod)"));
        assert_eq!(cell.value.as_ref().and_then(Value::number), Some(2.0));
        assert_eq!(cell.unit, None);
    }

    #[test]
    fn a_trigger_that_named_a_deleted_shape_names_none_and_is_otherwise_as_it_was() {
        let cell = trigger_cell(12);
        let after = trigger_after_delete(&cell, 12).expect("the call named shape 12");
        assert_eq!(after.formula.text(), Some("_XFTRIGGER(REF())"));
        assert_eq!((after.value.as_ref().and_then(Value::number), after.unit.as_deref(), after.error.is_none()), (Some(2.0), None, true));
        // Another shape's, and a formula of another kind, are not the glue's to rewrite.
        assert!(trigger_after_delete(&cell, 13).is_none());
        assert!(trigger_after_delete(&trigger_after_delete(&cell, 12).unwrap(), 12).is_none());
        assert!(trigger_after_delete(&Cell::with_formula("_XFTRIGGER(EventXFMod)", Some(Value::from_text("1"))), 12).is_none());
        assert!(trigger_after_delete(&Cell::constant(Value::from_text("2")), 12).is_none());
    }

    #[test]
    fn an_end_cell_holds_the_formula_and_the_place_as_a_length_with_no_unit() {
        let cell = end_cell(Which::End, 3.25);
        assert!(matches!(cell.formula, FormulaSlot::Local(_)));
        assert_eq!(cell.formula.text(), Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"));
        assert_eq!(cell.value.as_ref().and_then(Value::number), Some(3.25));
        assert_eq!(cell.unit, None);
    }
}
