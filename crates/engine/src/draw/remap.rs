//! Giving a copy the `ID`s of its own in the formulas that name shapes.
//!
//! A copy has new `ID`s, and a formula in a copy that names a shape that was copied with it must name the copy: a connector's
//! `PAR(PNT(Sheet.4!Connections.X1,...))`, its `BegTrigger` `_XFTRIGGER(Sheet.4!EventXFMod)`, a cell that reads `Sheet.4!Width`.
//! (ours; `Sheet.<id>` reaches a shape of the page by its `ID`): every `Sheet.<id>!` in the formula of a cell of a
//! copy, whose `<id>` is the `ID` of a shape copied, becomes `Sheet.<new>!`. Names no formula text is changed in: a reference by name
//! (`Process!Width`) is a reference to a peer by `NameU` and is kept as it is, so it goes on naming what it named (the first peer of that name); a
//! `Sheet.<id>` that follows another qualifier (`Pages[Site-2]!Sheet.9!EndY`) is on another page and is kept; and text inside a string or a quoted
//! name is not a reference. The rest of the formula is kept character for character. A reference to a shape not copied stays with the original on
//! the page the shapes were copied from; **on another page** it would reach whatever shape has that `ID` there, so such a cell is made the
//! constant of its cached value: the formula goes (a `GUARD` with it) and so does the cached error, and a cell with no cached value is left as it is.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{Cell, CellAddr, FormulaSlot, Sheet};

/// The longest `ID` in digits (`u32::MAX` has ten).
const MAX_DIGITS: usize = 10;

/// Whether the byte can be part of a name: a letter, a digit or `_`.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether a `Sheet.` that starts at `at` starts a qualifier: nothing before it joins it to a name or to another qualifier.
fn starts_qualifier(bytes: &[u8], at: usize) -> bool {
    match at.checked_sub(1).and_then(|before| bytes.get(before)) {
        None => true,
        Some(b) => !(is_name_byte(*b) || matches!(b, b'.' | b'!' | b'[' | b']' | b'\'' | b'"')),
    }
}

/// The `Sheet.<id>!` references in a formula that name a shape of the page the formula is on: where the digits start and end, and the `ID`. Not the
/// ones in a string or a quoted name, nor one that follows another qualifier or a name.
pub(crate) fn sheet_refs(source: &str) -> Vec<(usize, usize, u32)> {
    let bytes = source.as_bytes();
    let mut out: Vec<(usize, usize, u32)> = Vec::new();
    let (mut i, mut in_text, mut in_name) = (0_usize, false, false);
    while let Some(&b) = bytes.get(i) {
        match b {
            b'"' if !in_name => in_text = !in_text,
            b'\'' if !in_text => in_name = !in_name,
            b'S' | b's' if !in_text && !in_name && starts_qualifier(bytes, i) => {
                let digits_at = i + "Sheet.".len();
                if bytes.get(i..digits_at).is_some_and(|word| word.eq_ignore_ascii_case(b"sheet.")) {
                    let digits = bytes.get(digits_at..).map_or(0, |rest| rest.iter().take_while(|d| d.is_ascii_digit()).count());
                    let end = digits_at + digits;
                    let id = source.get(digits_at..end).filter(|_| (1..=MAX_DIGITS).contains(&digits)).and_then(|text| text.parse::<u32>().ok());
                    if let (Some(id), Some(&b'!')) = (id, bytes.get(end)) {
                        out.push((digits_at, end, id));
                        i = end;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The formula `source` with each `Sheet.<id>!` whose `<id>` is a key of `map` naming the value instead; `None` when none does.
pub(crate) fn rewrite_refs(source: &str, map: &HashMap<u32, u32>) -> Option<String> {
    let mut out = String::new();
    // The text before `copied` is in `out`.
    let mut copied = 0_usize;
    for (digits_at, end, id) in sheet_refs(source) {
        if let Some(new) = map.get(&id) {
            out.push_str(source.get(copied..digits_at)?);
            out.push_str(&new.to_string());
            copied = end;
        }
    }
    if copied == 0 {
        return None;
    }
    out.push_str(source.get(copied..)?);
    Some(out)
}

/// Whether the formula names, by `Sheet.<id>!`, a shape that is not a key of `map`: a shape that was not copied.
pub(crate) fn refers_outside(source: &str, map: &HashMap<u32, u32>) -> bool {
    sheet_refs(source).iter().any(|(_, _, id)| !map.contains_key(id))
}

/// Every cell of a sheet with its address: those directly under it, under each section, and in each row (a row told by its key).
pub(crate) fn cells(sheet: &Sheet) -> Vec<(CellAddr, &Cell)> {
    let mut out: Vec<(CellAddr, &Cell)> = Vec::new();
    for (name, cell) in &sheet.cells {
        out.push((CellAddr::direct(name.as_str()), cell));
    }
    for section in &sheet.sections {
        for (name, cell) in &section.cells {
            out.push((CellAddr::in_section(section.name.as_str(), section.ix, name.as_str()), cell));
        }
        for row in &section.rows {
            let Some(key) = row.key(section.name.as_str()) else { continue };
            for (name, cell) in &row.cells {
                out.push((CellAddr::in_row(section.name.as_str(), section.ix, key.clone(), name.as_str()), cell));
            }
        }
    }
    out
}

/// A sheet with these cells replaced, found by their addresses; the same sheet (one more reference to it) when there are none.
fn with_cells(sheet: &Arc<Sheet>, changed: Vec<(CellAddr, Cell)>) -> Arc<Sheet> {
    if changed.is_empty() {
        return Arc::clone(sheet);
    }
    let mut next = (**sheet).clone();
    for (addr, cell) in changed {
        // The cell is there: it was found by its address. A failure would be a sheet too large to hold the cell, which it already did.
        if next.set_cell(&addr, cell).is_err() {
            return Arc::clone(sheet);
        }
    }
    Arc::new(next)
}

/// The cell as the constant of its cached value, as `CellEdit::Value` makes a cell one: its formula taken away (a `GUARD` with it: a constant is not
/// guarded), **its error cleared** (the `E` of the last evaluation says what the formula gave, and a constant gives nothing: kept, it would be reported
/// for ever, and on the end of a connector would make a reroute skip the connector), its value, unit and the rest kept. `None` for a cell
/// with **no cached value** (a malformed cell): it has nothing to be a constant of, and an empty cell would read as 0, so its formula is
/// kept.
fn frozen(cell: &Cell) -> Option<Cell> {
    cell.value.as_ref()?;
    Some(Cell { formula: FormulaSlot::Absent, error: None, ..cell.clone() })
}

/// A sheet whose formulas name the copies in `map` where they named the shapes they were copied from. The same sheet (one more reference to it)
/// when no formula names one, which is what most shapes' are: an instance holds a pin and little else.
///
/// With `other_page` the copy is on another page, where `Sheet.<id>!` reaches the shape of the new page with that `ID` and not the original
/// A cell whose formula names a shape that was not copied is made the constant of its cached value, as the end of a connector
/// whose shape was not copied is, since it would be read against some other shape or none.
pub(crate) fn remap_sheet(sheet: &Arc<Sheet>, map: &HashMap<u32, u32>, other_page: bool) -> Arc<Sheet> {
    let changed: Vec<(CellAddr, Cell)> = cells(sheet)
        .into_iter()
        .filter_map(|(addr, cell)| {
            let FormulaSlot::Local(formula) = &cell.formula else { return None };
            if other_page
                && refers_outside(&formula.source, map)
                && let Some(constant) = frozen(cell)
            {
                return Some((addr, constant));
            }
            let source = rewrite_refs(&formula.source, map)?;
            let mut next = cell.clone();
            next.formula = Cell::with_formula(&source, None).formula;
            Some((addr, next))
        })
        .collect();
    with_cells(sheet, changed)
}

/// A sheet in which each cell whose formula names, by `Sheet.<id>!`, a shape in `gone` is the constant of its cached value (a group that
/// is taken apart is not there for a formula to read). The same sheet when no formula does.
pub(crate) fn freeze_refs(sheet: &Arc<Sheet>, gone: &HashSet<u32>) -> Arc<Sheet> {
    let changed: Vec<(CellAddr, Cell)> = cells(sheet)
        .into_iter()
        .filter_map(|(addr, cell)| {
            let FormulaSlot::Local(formula) = &cell.formula else { return None };
            if !sheet_refs(&formula.source).iter().any(|(_, _, id)| gone.contains(id)) {
                return None;
            }
            frozen(cell).map(|constant| (addr, constant))
        })
        .collect();
    with_cells(sheet, changed)
}

/// [`freeze_refs`] for the direct cells `names` of a sheet alone, each whose formula names, by `Sheet.<id>!`, a shape for which `named` holds: the
/// alignment cells of a shape released from a guide (`snap::hold`), whatever the call round the guide's (`GUARD(INTERSECTX(...))`,
/// `INTERSECTX(...)+0`). The same sheet when none does.
pub(crate) fn freeze_refs_of(sheet: &Arc<Sheet>, names: &[&str], named: impl Fn(u32) -> bool) -> Arc<Sheet> {
    let changed: Vec<(CellAddr, Cell)> = names
        .iter()
        .filter_map(|name| {
            let cell = sheet.cell(name)?;
            let FormulaSlot::Local(formula) = &cell.formula else { return None };
            if !sheet_refs(&formula.source).iter().any(|(_, _, id)| named(*id)) {
                return None;
            }
            frozen(cell).map(|constant| (CellAddr::direct(name), constant))
        })
        .collect();
    with_cells(sheet, changed)
}

/// The name of a copy: `Process.4` copied to the shape with `ID` 9 is `Process.9`, and a name that does not end in the `ID` it was copied from is kept.
/// `None` when the name is kept. Ours: no source read says what a copy is called (a nameless shape's default name is
/// `Sheet.<id>`, which this follows for a name that is that).
pub(crate) fn renamed(name: &str, old: u32, new: u32) -> Option<String> {
    let stem = name.strip_suffix(&old.to_string())?.strip_suffix('.')?;
    Some(format!("{stem}.{new}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn map() -> HashMap<u32, u32> {
        [(4, 40), (12, 120), (7, 7)].into_iter().collect()
    }

    #[test]
    fn a_sheet_reference_to_a_copied_shape_names_the_copy_and_the_rest_of_the_text_is_kept() {
        let map = map();
        assert_eq!(
            rewrite_refs("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))", &map).unwrap(),
            "PAR(PNT(Sheet.40!Connections.X1,Sheet.40!Connections.Y1))"
        );
        assert_eq!(rewrite_refs("_XFTRIGGER(Sheet.12!EventXFMod)", &map).unwrap(), "_XFTRIGGER(Sheet.120!EventXFMod)");
        assert_eq!(rewrite_refs("Sheet.4!Width + sheet.12!HEIGHT*2", &map).unwrap(), "Sheet.40!Width + sheet.120!HEIGHT*2");
        assert_eq!(rewrite_refs("Sheet.4!SheetRef()", &map).unwrap(), "Sheet.40!SheetRef()");
        // A shape that is kept under its own number still counts as a rewrite only if the number changed text: 7 stays 7.
        assert_eq!(rewrite_refs("Sheet.7!PinX", &map).unwrap(), "Sheet.7!PinX");
    }

    #[test]
    fn a_reference_to_a_shape_that_was_not_copied_or_that_is_not_a_reference_is_left_alone() {
        let map = map();
        for kept in [
            "Sheet.5!Width",
            "Width*2",
            "Pages[Site-2]!Sheet.4!EndY",
            "ThePage!Sheet.4!PinX",
            "\"Sheet.4!Width\"",
            "'Sheet.4'!Width",
            "MySheet.4!Width",
            "Sheet.4",
            "Sheet.4.5!Width",
            "Sheet.!Width",
            "Sheet.99999999999!Width",
            "Sheet.4x!Width",
            "Process!Width",
            "",
            "Sheet.",
            "Sheet",
        ] {
            assert_eq!(rewrite_refs(kept, &map), None, "{kept}");
        }
        // A quote closes a string, and the text after it is a formula again; a doubled quote inside one is a quote, not the end.
        assert_eq!(rewrite_refs("IF(A,\"say \"\"Sheet.4!\"\"\",Sheet.4!PinX)", &map).unwrap(), "IF(A,\"say \"\"Sheet.4!\"\"\",Sheet.40!PinX)");
        assert_eq!(rewrite_refs("héllo&Sheet.4!Width", &map).unwrap(), "héllo&Sheet.40!Width");
    }

    #[test]
    fn a_reference_to_a_shape_not_copied_is_found_and_on_another_page_its_cell_becomes_its_cached_value() {
        use diacraft_doc::CellName;
        let map = map();
        assert!(refers_outside("Sheet.5!Width*0.5", &map));
        assert!(refers_outside("PAR(PNT(Sheet.4!Connections.X1,Sheet.9!Connections.Y1))", &map), "one of two is enough");
        assert!(!refers_outside("Sheet.4!Width", &map));
        assert!(!refers_outside("Width*2", &map) && !refers_outside("Pages[Site-2]!Sheet.5!EndY", &map) && !refers_outside("\"Sheet.5!\"", &map));
        let mut sheet = Sheet::default();
        let value = |v: &str| Some(diacraft_doc::Value::from_text(v));
        sheet.cells.insert(CellName::new("Width"), Cell::with_formula("Sheet.5!Width*0.5", value("2")));
        sheet.cells.insert(CellName::new("Height"), Cell::with_formula("Sheet.4!Height", value("3")));
        sheet.cells.insert(CellName::new("PinX"), Cell::with_formula("2*3", value("6")));
        let sheet = Arc::new(sheet);
        // On the page they came from the reference to the shape not copied stays, and the one to a copy is the copy's.
        let same = remap_sheet(&sheet, &map, false);
        assert_eq!(same.cell("Width").unwrap().formula.text(), Some("Sheet.5!Width*0.5"));
        assert_eq!(same.cell("Height").unwrap().formula.text(), Some("Sheet.40!Height"));
        // On another page the cell that names a shape not copied is its cached value, and the others are as they were.
        let other = remap_sheet(&sheet, &map, true);
        let width = other.cell("Width").unwrap();
        assert_eq!((width.formula.text(), width.value.as_ref().map(|v| v.text())), (None, Some("2")));
        assert_eq!(other.cell("Height").unwrap().formula.text(), Some("Sheet.40!Height"));
        assert_eq!(other.cell("PinX").unwrap().formula.text(), Some("2*3"));
    }

    #[test]
    fn a_cell_that_names_a_shape_that_is_going_is_its_cached_value() {
        use diacraft_doc::CellName;
        let mut sheet = Sheet::default();
        let value = |v: &str| Some(diacraft_doc::Value::from_text(v));
        sheet.cells.insert(CellName::new("Width"), Cell::with_formula("Sheet.3!Width*0.25", value("1")).with_unit("IN"));
        sheet.cells.insert(CellName::new("Height"), Cell::with_formula("Sheet.4!Height+Sheet.3!Height", value("5")));
        sheet.cells.insert(CellName::new("PinX"), Cell::with_formula("Sheet.4!PinX", value("2")));
        let sheet = Arc::new(sheet);
        let gone: HashSet<u32> = [3].into_iter().collect();
        let after = freeze_refs(&sheet, &gone);
        let width = after.cell("Width").unwrap();
        assert_eq!((width.formula.text(), width.value.as_ref().map(|v| v.text()), width.unit.as_deref()), (None, Some("1"), Some("IN")));
        assert_eq!(after.cell("Height").unwrap().formula.text(), None, "one name of two is enough");
        assert_eq!(after.cell("PinX").unwrap().formula.text(), Some("Sheet.4!PinX"));
        assert!(Arc::ptr_eq(&freeze_refs(&sheet, &HashSet::new()), &sheet));
    }

    /// Three cells that name shape 5: one with a cached error, one with no cached value, one guarded.
    fn cells_that_freeze_three_ways() -> Arc<Sheet> {
        use diacraft_doc::CellName;
        let mut sheet = Sheet::default();
        let mut with_error = Cell::with_formula("Sheet.5!Width*0.5", Some(diacraft_doc::Value::from_text("2"))).with_unit("IN");
        with_error.error = Some(Arc::from("#REF!"));
        sheet.cells.insert(CellName::new("Width"), with_error);
        sheet.cells.insert(CellName::new("Height"), Cell::with_formula("Sheet.5!Height", None));
        sheet.cells.insert(CellName::new("LocPinX"), Cell::with_formula("GUARD(Sheet.5!Width*0.25)", Some(diacraft_doc::Value::from_text("1"))));
        Arc::new(sheet)
    }

    fn assert_frozen_three_ways(sheet: &Sheet, what: &str) {
        // The error of the last evaluation is cleared with the formula: a constant reports none (kept, it would be reported for ever).
        let width = sheet.cell("Width").unwrap();
        assert_eq!(
            (width.formula.text(), width.value.as_ref().map(|v| v.text()), width.error.as_deref(), width.unit.as_deref()),
            (None, Some("2"), None, Some("IN")),
            "{what}"
        );
        // No cached value, nothing to be a constant of: the formula stays, and no empty cell (which would read as 0) is made.
        let height = sheet.cell("Height").unwrap();
        assert_eq!((height.formula.text(), height.value.is_none()), (Some("Sheet.5!Height"), true), "{what}");
        // A guard goes with the formula: the cell is a plain constant.
        let guarded = sheet.cell("LocPinX").unwrap();
        assert_eq!((guarded.formula.text(), guarded.value.as_ref().map(|v| v.text()), guarded.is_guarded()), (None, Some("1"), false), "{what}");
    }

    #[test]
    fn a_frozen_cell_has_no_error_keeps_its_formula_when_it_has_no_value_and_loses_a_guard_with_its_formula() {
        let sheet = cells_that_freeze_three_ways();
        let map: HashMap<u32, u32> = HashMap::new();
        assert!(sheet.cell("LocPinX").unwrap().is_guarded());
        assert_frozen_three_ways(&remap_sheet(&sheet, &map, true), "on another page");
        assert_frozen_three_ways(&freeze_refs(&sheet, &[5].into_iter().collect()), "naming a group that goes");
        // On the page they came from nothing is frozen.
        assert!(Arc::ptr_eq(&remap_sheet(&sheet, &map, false), &sheet));
    }

    #[test]
    fn a_name_that_ends_in_the_id_it_was_copied_from_ends_in_the_new_one() {
        assert_eq!(renamed("Process.4", 4, 9).as_deref(), Some("Process.9"));
        assert_eq!(renamed("Sheet.14", 4, 9), None, "14 is not 4");
        assert_eq!(renamed("Process4", 4, 9), None);
        assert_eq!(renamed("Start", 4, 9), None);
        assert_eq!(renamed("A.B.4", 4, 90).as_deref(), Some("A.B.90"));
        assert_eq!(renamed(".4", 4, 9).as_deref(), Some(".9"));
    }

    #[test]
    fn a_sheet_that_names_no_copy_is_the_same_sheet_and_one_that_does_is_a_new_one_with_the_same_cells_otherwise() {
        use diacraft_doc::CellName;
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("PinX"), Cell::constant(diacraft_doc::Value::from_text("2")));
        let sheet = Arc::new(sheet);
        assert!(Arc::ptr_eq(&remap_sheet(&sheet, &map(), false), &sheet));
        assert!(Arc::ptr_eq(&remap_sheet(&sheet, &map(), true), &sheet));
        let mut with = (*sheet).clone();
        with.cells.insert(
            CellName::new("BeginX"),
            Cell::with_formula("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))", Some(diacraft_doc::Value::from_text("3"))),
        );
        let with = Arc::new(with);
        let next = remap_sheet(&with, &map(), false);
        assert!(!Arc::ptr_eq(&next, &with));
        let begin = next.cell("BeginX").unwrap();
        assert_eq!(begin.formula.text(), Some("PAR(PNT(Sheet.40!Connections.X1,Sheet.40!Connections.Y1))"));
        assert_eq!(begin.value.as_ref().map(|v| v.text()), Some("3"), "the cached value is kept");
        assert!(begin.formula.text().is_some() && matches!(&begin.formula, FormulaSlot::Local(f) if f.is_parsed()));
        assert_eq!(next.cell("PinX"), with.cell("PinX"));
        // The original is untouched.
        assert_eq!(with.cell("BeginX").unwrap().formula.text(), Some("PAR(PNT(Sheet.4!Connections.X1,Sheet.4!Connections.Y1))"));
    }
}
