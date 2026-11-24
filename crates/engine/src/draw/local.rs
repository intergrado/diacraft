//! A member that is an instance carries its master's geometry itself.
//!
//! A shape nested in a group that holds `Master` is in 172 files from 51 sources, and in the files measured the
//! application's own pages hold a local `Geometry` section on 94% of such shapes. An instance made by a drop holds **only what differs** (its pin),
//! which is right for a shape on the page and not what the files hold for a member. So when an instance becomes a member of a group
//! (`shape.group`), and when the members of a dropped group master are listed, the shape is given a local copy of every
//! `Geometry` section it has once its master is put together with it: the sections by `IX`, the rows by `IX` with their `T`,
//! the cells of each. **How a cell is held** follows what the files hold on instances (`Inh` on 507,641 cells, a
//! local formula on 34,533): a cell whose master has a formula is `F="Inh"` with the result it has on this shape as
//! its `V` (the formula is not copied: R26, and a formula that names `Sheet.<id>` is bound by the instance); a cell whose master holds a plain
//! value is a local constant. So the shape draws as it did, and goes on drawing the same when it is resized, since an `Inh` cell is worked out again
//! on the shape.
//!
//! **What this makes of a later edit of the master.** The shape now has the rows and the constants of its own: a row the master gains or loses, and a
//! number in a master's cell that holds no formula, no longer reach it. A formula of the master still does (the cell inherits it).
//!
//! **Only what is missing is written.** A section, row or cell the shape holds already (an override, a row it hides with `Del`, a row of its own) is left
//! exactly as it is, and a section or row the shape hides is not brought back. The result is therefore the merge R29 describes, written out: the effective
//! rows are unchanged, and a reader that does not know the master has all of them. Cells outside `Geometry` are not touched (the specifications give no count
//! of them on nested instances; the cells a size change makes local are `calc`'s).
//!
//! **Work is bounded.** A shape's copy costs its master's sections, rows and cells, so one command is charged for all of them before any is written,
//! and refused in words past [`MAX_LOCALISED_WORK`] (the largest `Geometry` section the sample-file run found holds 31,273 rows; a drawing with thousands of
//! such shapes is the case the cap is for). Plans are made a chunk of shapes at a time, so what is held beside the document is bounded too.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, Document, FormulaSlot, MAX_SHAPE_DEPTH, Resolved, Resolver, Row, RowKey, Section, ShapeRef, Sheet, SheetRef,
};

use super::{Problem, shape_mut};

/// The most sections, rows and cells one command may copy into shapes, counted over the master's `Geometry` and what the shapes hold already: one
/// million (about 40,000 shapes of 25 each).
pub const MAX_LOCALISED_WORK: usize = 1_000_000;

/// How much work is planned before it is written: what is held beside the document at one time.
const CHUNK: usize = 50_000;

const GEOMETRY: &str = "Geometry";

/// A cell to add to a row or a section.
type NewCell = (CellName, Cell);

/// What is missing from one row.
struct RowPlan {
    key: RowKey,
    /// The row's `T`, when the row has none of its own.
    row_type: Option<Arc<str>>,
    cells: Vec<NewCell>,
}

/// What is missing from one `Geometry` section.
struct SectionPlan {
    ix: Option<u32>,
    cells: Vec<NewCell>,
    rows: Vec<RowPlan>,
}

/// What is missing from one shape.
struct ShapePlan {
    path: Vec<u32>,
    sections: Vec<SectionPlan>,
}

fn charge(left: &mut usize, n: usize) -> Result<(), Problem> {
    *left = left.checked_sub(n).ok_or_else(|| {
        Problem::failed(format!(
            "copying the geometry of the masters into these shapes would write more than {MAX_LOCALISED_WORK} sections, rows and cells: use fewer shapes, or shapes with less geometry"
        ))
    })?;
    Ok(())
}

/// Charges the `Geometry` sections of a sheet: a section, each of its rows, and each cell. A section of another name costs one, so a sheet of
/// thousands of sections is not free to look through.
fn charge_sheet(sheet: &Sheet, left: &mut usize) -> Result<(), Problem> {
    for section in &sheet.sections {
        charge(left, 1)?;
        if section.name.as_str() != GEOMETRY {
            continue;
        }
        charge(left, section.cells.len())?;
        for row in &section.rows {
            charge(left, 1 + row.cells.len())?;
        }
    }
    Ok(())
}

/// The first `Geometry` section of each `IX` of a sheet, as every lookup finds it.
fn geometry_sections(sheet: &Sheet) -> HashMap<Option<u32>, &Arc<Section>> {
    let mut found = HashMap::new();
    for section in sheet.sections.iter().filter(|s| s.name.as_str() == GEOMETRY) {
        found.entry(section.ix).or_insert(section);
    }
    found
}

/// A cell as the shape is to hold it, from what it resolves to now: the master's formula stays inherited (`F="Inh"`) with the result it has here as its
/// `V`, a plain value is held as a constant, and `No Formula` is kept. `None` for a cell that is found nowhere or says nothing.
fn held(resolved: &Resolved<'_>) -> Option<Cell> {
    let formula = match resolved.formula.cell.map(|cell| &cell.formula) {
        Some(FormulaSlot::Local(_) | FormulaSlot::Inherit) => FormulaSlot::Inherit,
        Some(FormulaSlot::NoFormula) => FormulaSlot::NoFormula,
        Some(FormulaSlot::Absent) | None => FormulaSlot::Absent,
    };
    let value = resolved.value.cell.and_then(|cell| cell.value.clone());
    if matches!(formula, FormulaSlot::Absent) && value.is_none() {
        return None;
    }
    let unit = resolved.value.cell.and_then(|cell| cell.unit.clone()).or_else(|| resolved.formula.cell.and_then(|cell| cell.unit.clone()));
    Some(Cell { formula, value, unit, ..Cell::default() })
}

/// What the shape at `at` lacks of the `Geometry` it has by inheritance; `None` when it inherits none or lacks nothing. With the units of work it is.
fn plan_shape(res: &Resolver<'_>, at: &ShapeRef) -> Option<(ShapePlan, usize)> {
    let step = res.master_step(at).ok()??;
    let local = &res.shape(at)?.sheet;
    let master = &res.shape(&step.shape)?.sheet;
    let sheet = SheetRef::Shape(at.clone());
    let (local_at, master_at) = (geometry_sections(local), geometry_sections(master));
    let mut sections = Vec::new();
    let mut units = 0_usize;
    for key in res.effective_sections(&sheet, GEOMETRY)? {
        let held_here = local_at.get(&key.ix).copied();
        let mut cells: Vec<NewCell> = Vec::new();
        if let Some(of_master) = master_at.get(&key.ix) {
            for name in of_master.cells.keys() {
                if held_here.is_some_and(|section| section.cells.contains_key(name)) {
                    continue;
                }
                let addr = CellAddr::in_section(GEOMETRY, key.ix, name.as_str());
                if let Some(cell) = res.resolve(&sheet, &addr).and_then(|resolved| held(&resolved)) {
                    cells.push((name.clone(), cell));
                }
            }
        }
        let mut rows = Vec::new();
        for effective in res.effective_rows(&sheet, &key)? {
            let local_row = held_here.and_then(|section| section.row(&effective.key));
            let mut missing: Vec<NewCell> = Vec::new();
            for (name, resolved) in &effective.cells {
                if local_row.is_some_and(|row| row.cells.contains_key(name)) {
                    continue;
                }
                if let Some(cell) = held(resolved) {
                    missing.push((name.clone(), cell));
                }
            }
            let row_type = effective.row_type.clone().filter(|_| local_row.is_none_or(|row| row.row_type.is_none()));
            if local_row.is_some() && missing.is_empty() && row_type.is_none() {
                continue;
            }
            units = units.saturating_add(1 + missing.len());
            rows.push(RowPlan { key: effective.key, row_type, cells: missing });
        }
        if cells.is_empty() && rows.is_empty() {
            continue;
        }
        units = units.saturating_add(1 + cells.len());
        sections.push(SectionPlan { ix: key.ix, cells, rows });
    }
    if sections.is_empty() {
        return None;
    }
    Some((ShapePlan { path: at.path.clone(), sections }, units))
}

/// A row as a plan makes it: keyed by its `IX` (or its `N`, for a key no `Geometry` row has).
fn new_row(plan: RowPlan) -> Row {
    let mut row = Row { row_type: plan.row_type, cells: plan.cells.into_iter().collect(), ..Row::default() };
    match plan.key {
        RowKey::Ix(ix) => row.ix = Some(ix),
        RowKey::Name(name) => row.name = Some(name),
    }
    row
}

/// Writes a plan into the shape's sheet. A section the shape has is added to and the rows it has are kept in their place; the cells written are the
/// ones the plan found missing.
fn apply(doc: &mut Document, page: u32, plan: ShapePlan) -> Result<(), Problem> {
    let shape = shape_mut(doc, page, &plan.path).ok_or_else(|| Problem::failed("a shape went away"))?;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let mut at: HashMap<Option<u32>, usize> = HashMap::new();
    for (place, section) in sheet.sections.iter().enumerate() {
        if section.name.as_str() == GEOMETRY {
            at.entry(section.ix).or_insert(place);
        }
    }
    for wanted in plan.sections {
        let existed = at.contains_key(&wanted.ix);
        let place = match at.get(&wanted.ix) {
            Some(place) => *place,
            None => {
                sheet.sections.push(Arc::new(Section::new(GEOMETRY, wanted.ix)));
                let place = sheet.sections.len().saturating_sub(1);
                at.insert(wanted.ix, place);
                place
            }
        };
        let section = sheet.sections.get_mut(place).map(Arc::make_mut).ok_or_else(|| Problem::failed("a section went away"))?;
        for (name, cell) in wanted.cells {
            section.cells.insert(name, cell);
        }
        // The rows the section holds, by key: one pass, so that a section of very many rows is not searched for each row.
        let mut rows_at: HashMap<RowKey, usize> = HashMap::new();
        if existed {
            for (place, row) in section.rows.iter().enumerate() {
                if let Some(key) = row.key(GEOMETRY) {
                    rows_at.entry(key).or_insert(place);
                }
            }
        }
        let mut added = false;
        for row in wanted.rows {
            match rows_at.get(&row.key).copied() {
                Some(place) => {
                    let held = section.rows.get_mut(place).map(Arc::make_mut).ok_or_else(|| Problem::failed("a row went away"))?;
                    if held.row_type.is_none() {
                        held.row_type = row.row_type;
                    }
                    for (name, cell) in row.cells {
                        held.cells.insert(name, cell);
                    }
                }
                None => {
                    section.rows.push(Arc::new(new_row(row)));
                    added = true;
                }
            }
        }
        // A section the shape had keeps its rows in `IX` order with the new ones among them, as a file lists them.
        if existed && added {
            section.rows.sort_by_key(|row| row.ix.unwrap_or(u32::MAX));
        }
    }
    sheet.check_limits()?;
    Ok(())
}

/// Charges the work of copying into `shapes`, before any of it is done: `Err` in words when it is more than [`MAX_LOCALISED_WORK`]. A shape that
/// inherits no geometry costs nothing. Cheap whatever the geometry: it stops at the cap.
pub(crate) fn charge_all(doc: &Document, shapes: &[ShapeRef]) -> Result<(), Problem> {
    let res = Resolver::new(doc);
    let mut left = MAX_LOCALISED_WORK;
    for at in shapes {
        let Ok(Some(step)) = res.master_step(at) else { continue };
        if let Some(shape) = res.shape(&step.shape) {
            charge_sheet(&shape.sheet, &mut left)?;
        }
        if let Some(shape) = res.shape(at) {
            charge_sheet(&shape.sheet, &mut left)?;
        }
    }
    Ok(())
}

/// Gives each shape of `shapes` (on `page`) that is an instance, or stands for a shape of a master, a local copy of the `Geometry` it inherits
/// A shape that inherits none is left alone. → how many shapes were written to. All or nothing as far as the caller goes: the
/// caller edits a copy of the document, and the work is charged before anything is written ([`charge_all`]).
pub(crate) fn localise_geometry(doc: &mut Document, page: u32, shapes: &[ShapeRef]) -> Result<usize, Problem> {
    charge_all(doc, shapes)?;
    let (mut written, mut next) = (0_usize, 0_usize);
    while next < shapes.len() {
        let mut plans: Vec<ShapePlan> = Vec::new();
        let mut units = 0_usize;
        {
            let res = Resolver::new(doc);
            while units < CHUNK
                && let Some(at) = shapes.get(next)
            {
                next = next.saturating_add(1);
                if let Some((plan, work)) = plan_shape(&res, at) {
                    units = units.saturating_add(work);
                    plans.push(plan);
                }
            }
        }
        for plan in plans {
            apply(doc, page, plan)?;
            written = written.saturating_add(1);
        }
    }
    Ok(written)
}

/// Every shape below the one at `at`, to any depth the model allows: their places, parents first. Iterative.
pub(crate) fn below(doc: &Document, at: &ShapeRef) -> Vec<ShapeRef> {
    let mut found = Vec::new();
    let mut stack = vec![at.clone()];
    while let Some(parent) = stack.pop() {
        let Some(shape) = doc.shape(&parent) else { continue };
        for k in 0..shape.children.len() {
            let Ok(k) = u32::try_from(k) else { break };
            let child = parent.child(k);
            found.push(child.clone());
            if child.path.len() < MAX_SHAPE_DEPTH {
                stack.push(child);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn sheet_with(rows: usize, other_sections: usize) -> Sheet {
        let mut section = Section::new(GEOMETRY, Some(0));
        for k in 0..rows {
            let mut row = Row { ix: u32::try_from(k).ok(), ..Row::default() };
            row.cells.insert(CellName::new("X"), Cell::default());
            row.cells.insert(CellName::new("Y"), Cell::default());
            section.rows.push(Arc::new(row));
        }
        let mut sheet = Sheet::default();
        sheet.sections.push(Arc::new(section));
        sheet.sections.extend((0..other_sections).map(|_| Arc::new(Section::new("Connection", None))));
        sheet
    }

    #[test]
    fn the_work_of_a_sheet_is_a_section_each_row_and_each_cell_and_one_for_a_section_of_another_name() {
        let mut left = 1_000;
        charge_sheet(&sheet_with(10, 0), &mut left).unwrap();
        assert_eq!(left, 1_000 - (1 + 10 * 3), "a section, and a row and its two cells for each row");
        let mut left = 1_000;
        charge_sheet(&sheet_with(0, 7), &mut left).unwrap();
        assert_eq!(left, 1_000 - (1 + 7), "a section of another name costs one, so a sheet of very many is not free to look through");
    }

    #[test]
    fn the_work_past_what_is_left_is_refused_in_words_and_stops_at_it() {
        let mut left = 20;
        let e = charge_sheet(&sheet_with(10_000, 0), &mut left).unwrap_err();
        assert!(e.message.contains("more than 1000000") && e.message.contains("fewer shapes"), "{}", e.message);
        assert!(!e.bad_params, "the drawing is at fault, not the call");
        assert!(left < 20, "the count went as far as it could, and no further than the sheet's own sections");
    }

    #[test]
    fn a_row_is_keyed_by_its_ix_or_by_its_name_and_holds_the_type_and_cells_of_its_plan() {
        let cells = vec![(CellName::new("X"), Cell::default())];
        let by_ix = new_row(RowPlan { key: RowKey::Ix(4), row_type: Some(Arc::from("LineTo")), cells: cells.clone() });
        assert_eq!((by_ix.ix, by_ix.name, by_ix.row_type.as_deref(), by_ix.cells.len()), (Some(4), None, Some("LineTo"), 1));
        let by_name = new_row(RowPlan { key: RowKey::name("Row_1"), row_type: None, cells });
        assert_eq!((by_name.ix, by_name.name.as_deref(), by_name.row_type), (None, Some("Row_1"), None));
    }
}
