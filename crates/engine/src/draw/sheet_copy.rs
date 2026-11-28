//! Copy, then insert (follow-up, the owner's ruling of 2026-11-26): a middle insert into a section that an instance inherits from its master or its style.
//!
//! An indexed row is told by its number, and a sheet that inherits a section holds its rows by the numbers its master gives them, so a row that moved under another number would be
//! read together with whatever the master holds at that number (rows merge cell by cell). So **the instance first takes its own copy of the rows**, and the insert
//! then goes as it goes into a section the sheet holds itself:
//!
//! * **The copy** ([`plan_copy`], [`apply_copy`]) writes every effective row of the section into the sheet as a row of its own, with the cells it reads (own or inherited, merged) as
//!   cells of its own: **a formula is copied as its text** (what the master's row says, worked out on the instance as it was), its cached value and unit with it, and the cells of the
//!   section itself (Geometry's `NoFill` and the rest). A row the sheet dropped (`Del`) stays dropped. A cell the master's formula gave is *not* `F="Inh"` (as the copy of a
//!   group member's geometry holds it): an inherited cell reads the master's cell **at its own number**, and after the rows move that is another row's.
//! * **What it does to a later edit of the master.** The instance holds those rows itself now: a number or a formula the master's rows later get no longer reaches this instance (it still
//!   reaches every instance that has no copy), and a row the master later adds at a number the instance has used shows through there. That is the ruling: the instance's rows are its own.
//! * **A formula that names a shape of the master** (`Sheet.<id>!Width`) is bound by the instance when it is the master's and would name another shape when it is the
//!   sheet's own: such a row cannot be copied, and the insert is refused in words (the end is still the place).
//! * **No cell of the master shows through** ([`mask_leaks`]): after the rows are moved, the row at each number the master holds is given every cell the master's row there holds that it lacks,
//!   at its default, as a cell of its own (an arc's `A` is not read on the line that now stands at the arc's number). The rows moved up, so every number the master holds is still held by a
//!   row or a deleted row of the sheet's, and no `Del` row is made for the insert.
//! * **Costs, as counts**: the copy costs the cells of the rows ([`MAX_COPIED_CELLS`], refused in words past it, before anything is written); the rows are found in one pass over the section
//!   and the sheet's own rows by key in one map.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, Document, FormulaSlot, Resolved, Resolver, Row, RowKey, RowShape, Section, SectionKey, SheetRef, cell_names,
    cells_changed, ix_to_row_number,
};

use super::Problem;
use super::sheet_rows::{default_cell, plain};

/// The most cells one copy writes. A section holds at most 320,000 rows of at most 4,096 cells, which is more than a drawing can carry; this holds a copy to what a drawing of 30,000
/// shapes of Geometry can.
pub const MAX_COPIED_CELLS: usize = 2_000_000;

/// What the instance takes of the section, and what it must keep of the master's.
pub(super) struct CopyPlan {
    /// The effective rows, each as a row of the sheet's own.
    pub rows: Vec<Row>,
    /// The cells of the section itself the sheet does not hold, to be held.
    pub flags: Vec<(CellName, Cell)>,
    /// By `IX`: the names of the cells the layers that give the section hold in the row at that number, a row a nearer layer deletes not counted.
    pub masks: HashMap<u32, Vec<CellName>>,
    /// The cells the copy writes.
    pub cells: usize,
}

/// The cell the sheet holds of its own for what `found` resolves to: the cell that gives the formula (its text, its unit), with the value (and the error) of the cell that gives the value.
/// `None` for a cell that says nothing.
fn copied(found: &Resolved<'_>) -> Option<Cell> {
    let source = found.formula.cell.or(found.value.cell)?;
    let mut cell = source.clone();
    if let Some(valued) = found.value.cell {
        cell.value = valued.value.clone();
        cell.error = valued.error.clone();
        if cell.unit.is_none() {
            cell.unit = valued.unit.clone();
        }
    }
    // The cell that gives the formula ends the search for one, so it holds none to inherit; a stray marker is no formula here.
    if matches!(cell.formula, FormulaSlot::Inherit) {
        cell.formula = FormulaSlot::Absent;
    }
    Some(cell)
}

/// The cells the layers that give the section hold, by the `IX` of the row (nearest layer first: a row it deletes is not counted from the layers behind it).
fn layer_masks(others: &[&Arc<Section>], element: &str) -> HashMap<u32, Vec<CellName>> {
    let mut names: HashMap<u32, BTreeSet<CellName>> = HashMap::new();
    let mut blocked: HashSet<RowKey> = HashSet::new();
    for section in others {
        let mut deleted_here: Vec<RowKey> = Vec::new();
        for row in &section.rows {
            let Some(key) = row.key(element) else { continue };
            if blocked.contains(&key) {
                continue;
            }
            if row.del {
                deleted_here.push(key);
            } else if let RowKey::Ix(ix) = key {
                names.entry(ix).or_default().extend(row.cells.keys().cloned());
            }
        }
        blocked.extend(deleted_here);
    }
    names.into_iter().map(|(ix, set)| (ix, set.into_iter().collect())).collect()
}

/// Plans the copy of the section of `sheet` into the sheet: the effective rows `effective`, as given by the layers `others` and the sheet's own rows. `limit` is the most cells it may write.
pub(super) fn plan_copy(
    res: &Resolver<'_>,
    sheet: &SheetRef,
    key: &SectionKey,
    effective: &[RowShape],
    others: &[&Arc<Section>],
    spelled: &str,
    limit: usize,
) -> Result<CopyPlan, Problem> {
    let element = key.name.as_str();
    let geometry = element == "Geometry";
    let cells: usize = effective.iter().map(|r| r.cells.len()).sum::<usize>().saturating_add(others.iter().map(|s| s.cells.len()).sum());
    if cells > limit {
        return Err(Problem::failed(format!(
            "copying the {} rows of {spelled} into the shape would write {cells} cells, and a copy is held to {limit}: add the row at the end",
            effective.len()
        )));
    }
    let mut rows = Vec::with_capacity(effective.len());
    for shape in effective {
        let mut row = Row { row_type: shape.row_type.clone(), ..Row::default() };
        match &shape.key {
            RowKey::Ix(ix) => row.ix = Some(*ix),
            RowKey::Name(name) => row.name = Some(Arc::clone(name)),
        }
        for name in &shape.cells {
            let addr = CellAddr { section: Some(key.clone()), row: Some(shape.key.clone()), cell: name.clone() };
            let Some(cell) = res.resolve(sheet, &addr).as_ref().and_then(copied) else { continue };
            if let FormulaSlot::Local(formula) = &cell.formula
                && let Some((_, _, id)) = super::sheet_refs(&formula.source).first().copied()
            {
                let number = match &shape.key {
                    RowKey::Ix(ix) => ix_to_row_number(geometry, *ix).map_or_else(|| ix.to_string(), |n| n.to_string()),
                    RowKey::Name(name) => name.to_string(),
                };
                return Err(Problem::failed(format!(
                    "row {number} of {spelled} names a shape of the master (Sheet.{id}) in {}: the instance cannot take a copy of it, which would name another shape: add the row at the end",
                    name.as_str()
                )));
            }
            row.cells.insert(name.clone(), cell);
        }
        rows.push(row);
    }
    // The cells of the section itself that the sheet lacks: the layers' own, merged.
    let mut flags: Vec<(CellName, Cell)> = Vec::new();
    let held = res.sheet(sheet).and_then(|s| s.section(key)).map(|s| s.cells.keys().cloned().collect::<HashSet<_>>()).unwrap_or_default();
    let flag_names: BTreeSet<CellName> = others.iter().flat_map(|s| s.cells.keys().cloned()).filter(|n| !held.contains(n)).collect();
    for name in flag_names {
        let addr = CellAddr { section: Some(key.clone()), row: None, cell: name.clone() };
        if let Some(cell) = res.resolve(sheet, &addr).as_ref().and_then(copied) {
            flags.push((name, cell));
        }
    }
    let written = rows.iter().map(|r| r.cells.len()).sum::<usize>() + flags.len();
    Ok(CopyPlan { rows, flags, masks: layer_masks(others, element), cells: written })
}

/// Writes the copy into the sheet's own section (made if the sheet holds none): each effective row replaces the sheet's own row at its number, or is added; the rows the sheet dropped
/// are left as they are.
pub(super) fn apply_copy(doc: &mut Document, sheet: &SheetRef, key: &SectionKey, plan: &CopyPlan) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    let section = target.section_mut(key).map_err(|e| Problem::failed(e.to_string()))?;
    if !plan.flags.is_empty() {
        let before = cell_names(&section.passthrough, &section.cells);
        for (name, cell) in &plan.flags {
            section.cells.insert(name.clone(), cell.clone());
        }
        cells_changed(&mut section.passthrough, &before, &section.cells);
    }
    let element = key.name.as_str();
    let mut at: HashMap<RowKey, usize> = HashMap::new();
    for (place, row) in section.rows.iter().enumerate() {
        if let Some(held) = row.key(element) {
            at.entry(held).or_insert(place);
        }
    }
    let mut added: Vec<Row> = Vec::new();
    for row in &plan.rows {
        let Some(held) = row.key(element) else { continue };
        match at.get(&held).copied().and_then(|place| section.rows.get_mut(place)) {
            Some(slot) => {
                let own = Arc::make_mut(slot);
                let before = cell_names(&own.passthrough, &own.cells);
                own.cells = row.cells.clone();
                cells_changed(&mut own.passthrough, &before, &own.cells);
                own.row_type = row.row_type.clone();
            }
            None => added.push(row.clone()),
        }
    }
    put_in_order(section, added);
    Ok(())
}

/// The key rows are put in order by: the number, and a row with none last.
fn order_of(row: &Row) -> (bool, Option<u32>) {
    (row.ix.is_none(), row.ix)
}

/// Puts `added` among the section's rows in the order of their numbers, after the rows that hold the same number, in one merge of the two lists, and moves the nodes that stood among
/// the section's children with the rows that were there (a node goes on following the row it followed, ahead of the rows put in after it). A section whose rows are not in order (a row
/// a cell write made for an instance goes after the rows it holds, whatever its number) is put in order first (`Section::sort_rows_by_key`), and the nodes follow the rows that change
/// places.
fn put_in_order(section: &mut Section, mut added: Vec<Row>) {
    if added.is_empty() {
        return;
    }
    added.sort_by_key(order_of);
    section.sort_rows_by_key(order_of);
    let old = std::mem::take(&mut *section.rows);
    let old_len = old.len();
    let mut merged: Vec<Arc<Row>> = Vec::with_capacity(old_len.saturating_add(added.len()));
    let mut places: Vec<usize> = Vec::with_capacity(added.len());
    let mut new_rows = added.into_iter().map(Arc::new).peekable();
    for row in old {
        while let Some(next) = new_rows.next_if(|next| order_of(next) < order_of(&row)) {
            places.push(merged.len());
            merged.push(next);
        }
        merged.push(row);
    }
    for next in new_rows {
        places.push(merged.len());
        merged.push(next);
    }
    *section.rows = merged;
    section.passthrough.children_inserted(section.cells.len(), old_len, &places, true);
}

/// After the rows are moved: gives the row at each number the layers hold every cell they hold in the row there that it lacks, as a cell of its own at its default, so that the layers'
/// cell is not read through it.
pub(super) fn mask_leaks(doc: &mut Document, sheet: &SheetRef, key: &SectionKey, plan: &CopyPlan) -> Result<(), Problem> {
    let mut target = doc.counted_mut(sheet).ok_or_else(|| Problem::failed("the sheet went away"))?;
    let section = target.section_mut(key).map_err(|e| Problem::failed(e.to_string()))?;
    let element = key.name.as_str();
    for slot in section.rows.iter_mut() {
        if slot.del {
            continue;
        }
        let Some(RowKey::Ix(ix)) = slot.key(element) else { continue };
        let Some(names) = plan.masks.get(&ix) else { continue };
        if names.iter().all(|name| slot.cells.contains_key(name)) {
            continue;
        }
        let own = Arc::make_mut(slot);
        let before = cell_names(&own.passthrough, &own.cells);
        for name in names {
            if !own.cells.contains_key(name) {
                own.cells.insert(name.clone(), default_cell(element, name.as_str()).unwrap_or_else(|| plain("0")));
            }
        }
        cells_changed(&mut own.passthrough, &before, &own.cells);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::Value as Held;

    use super::*;

    fn section_of(rows: &[(u32, &[&str], bool)]) -> Arc<Section> {
        let mut section = Section::new("Scratch", None);
        for (ix, cells, del) in rows {
            let mut row = Row { ix: Some(*ix), del: *del, ..Row::default() };
            for name in *cells {
                row.cells.insert(CellName::new(name), Cell::constant(Held::from_text("1")));
            }
            section.rows.push(Arc::new(row));
        }
        Arc::new(section)
    }

    #[test]
    fn the_masks_are_the_cells_each_number_holds_and_a_row_a_nearer_layer_deletes_holds_none() {
        let near = section_of(&[(0, &["A", "B"], false), (1, &[], true)]);
        let far = section_of(&[(0, &["C"], false), (1, &["A"], false), (2, &["A", "D"], false)]);
        let masks = layer_masks(&[&near, &far], "Scratch");
        let names = |ix: u32| masks.get(&ix).map(|v| v.iter().map(|n| n.as_str().to_owned()).collect::<Vec<_>>());
        assert_eq!(names(0), Some(vec!["A".to_owned(), "B".to_owned(), "C".to_owned()]));
        assert_eq!(names(1), None, "the nearer layer deletes the row, so the one behind it is not read");
        assert_eq!(names(2), Some(vec!["A".to_owned(), "D".to_owned()]));
    }

    #[test]
    fn a_copy_past_the_limit_is_refused_in_words_before_any_cell_is_made() {
        let doc = Document::new_blank();
        let res = Resolver::new(&doc);
        let shapes: Vec<RowShape> =
            (0..10).map(|ix| RowShape { key: RowKey::Ix(ix), row_type: None, cells: vec![CellName::new("A"), CellName::new("B")] }).collect();
        let key = SectionKey::new("Scratch", None);
        let e = plan_copy(&res, &SheetRef::Document, &key, &shapes, &[], "Scratch", 19).err().unwrap();
        assert!(
            e.message.contains("copying the 10 rows of Scratch") && e.message.contains("20 cells") && e.message.contains("held to 19"),
            "{}",
            e.message
        );
        // At the limit the rows are planned (the sheet holds none of the cells, so each is skipped as nothing resolves).
        let plan = plan_copy(&res, &SheetRef::Document, &key, &shapes, &[], "Scratch", 20).unwrap();
        assert_eq!(plan.rows.len(), 10);
    }
}
