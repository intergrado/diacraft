//! The shape's own `Connection` section (design §5).
//!
//! A shape that inherits its connection points from its master, wholly or in part, **takes the section as its own** when a doubling or a halving changes it: every row it will hold
//! is written as a row of its own with every cell (the formula as it resolves, with its value and unit), and every inherited row it will not hold is a row of its own that is deleted
//! (`Del`), so that nothing of the master's section shows through. A row that moved cannot stay inherited: an inherited cell is read from the master's row
//! of the same number, which after a renumbering is another point. A doubling, which only appends, needs no deleted row; a halving that takes inherited rows away does.
//! Afterwards an edit of the master's points no longer reaches the shape.
//!
//! The rows a call writes are counted by the caller before it writes them (`MAX_ROWS_WRITTEN`), and the shape's own section is rebuilt in one go: nothing is searched for row by row.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellName, Document, FormulaSlot, Owner, Resolved, Row, RowKey, Rows, Section, SectionKey, ShapeRef, SheetRef, Source, section_class,
};

use crate::draw::{Problem, Reader, shape_mut};

/// The element name of the section.
pub(super) const ELEMENT: &str = "Connection";

/// What the layers a shape inherits its points from hold: the key of each row they give (a row a nearer layer deletes is not given) and the names of the cells that row holds.
#[derive(Debug, Default)]
pub(super) struct Inherited {
    pub keys: Vec<RowKey>,
    pub masks: HashMap<RowKey, Vec<CellName>>,
}

/// The cell the shape holds of its own for what `found` resolves to: the cell that gives the formula (its text and unit), with the value (and the error) of the cell that gives the
/// value. `None` for a cell that says nothing.
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
    // The cell that gives the formula ends the search for one, so it holds none to inherit: a stray marker is no formula here.
    if matches!(cell.formula, FormulaSlot::Inherit) {
        cell.formula = FormulaSlot::Absent;
    }
    Some(cell)
}

/// Every effective row of the shape's `Connection` section as a row of its own, in the section's order, each with every cell it reads: its `IX` or name set as it is now. A row the
/// shape holds itself keeps what is not a cell (its type, its local name, what a file kept of it).
pub(super) fn current_rows(reader: &Reader<'_>, at: &ShapeRef, held: usize) -> Vec<Row> {
    let sheet = SheetRef::Shape(at.clone());
    let key = SectionKey::new(ELEMENT, None);
    let Some(Ok(effective)) = reader.resolver().effective_rows_within(&sheet, &key, held) else { return Vec::new() };
    let own: HashMap<RowKey, Arc<Row>> = reader
        .doc()
        .shape(at)
        .and_then(|shape| {
            shape
                .sheet
                .section(&key)
                .map(|section| section.rows.iter().filter(|r| !r.del).filter_map(|r| Some((r.key(ELEMENT)?, Arc::clone(r)))).collect())
        })
        .unwrap_or_default();
    effective
        .iter()
        .map(|row| {
            let mut made = own.get(&row.key).map_or_else(Row::default, |held| (**held).clone());
            made.cells.clear();
            match &row.key {
                RowKey::Ix(ix) => (made.ix, made.name) = (Some(*ix), None),
                RowKey::Name(name) => (made.ix, made.name) = (None, Some(Arc::clone(name))),
            }
            made.row_type = row.row_type.clone();
            made.del = false;
            for (name, found) in &row.cells {
                if let Some(cell) = copied(found) {
                    made.cells.insert(name.clone(), cell);
                }
            }
            made
        })
        .collect()
}

/// The rows the layers other than the shape's own give, found as the shape resolves them: nearest layer first, a row deleted in a nearer layer not given by the ones behind it, and
/// the layers behind a layer that deletes the whole section not at all. A section the shape has deleted whole hides nothing from this: what the master gives is what a new
/// section of the shape's own must mask.
pub(super) fn inherited(reader: &Reader<'_>, at: &ShapeRef) -> Inherited {
    let sheet = SheetRef::Shape(at.clone());
    let key = SectionKey::new(ELEMENT, None);
    let mut out = Inherited::default();
    let Some(layers) = reader.resolver().lookup_layers(&sheet, section_class(ELEMENT)) else { return out };
    let mut blocked: HashSet<RowKey> = HashSet::new();
    for (source, layer) in layers {
        if matches!(source, Source::Local) {
            continue;
        }
        let Some(section) = layer.section(&key) else { continue };
        if section.del {
            break;
        }
        let mut deleted_here: Vec<RowKey> = Vec::new();
        for row in &section.rows {
            let Some(row_key) = row.key(ELEMENT) else { continue };
            if blocked.contains(&row_key) {
                continue;
            }
            if row.del {
                deleted_here.push(row_key);
                continue;
            }
            let names = out.masks.entry(row_key.clone()).or_default();
            for name in row.cells.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            if !out.keys.contains(&row_key) {
                out.keys.push(row_key);
            }
        }
        blocked.extend(deleted_here);
    }
    out
}

/// Makes the shape's own `Connection` section the rows `rows` (each with its final key) and, for every row `inherited` gives that none of them holds, a deleted row. The section's
/// own cells are kept as the shape has them and a section it deleted is its own again.
pub(super) fn write_own(doc: &mut Document, at: &ShapeRef, mut rows: Vec<Row>, inherited: &Inherited) -> Result<(), Problem> {
    let Owner::Page(page) = at.owner else { return Err(Problem::failed("connection points are changed on a shape of a page")) };
    // No cell of a layer shows through a row of the shape's own: the row at each key the layers hold has every cell they hold there.
    let kept: HashSet<RowKey> = rows.iter().filter_map(|r| r.key(ELEMENT)).collect();
    for row in &mut rows {
        let Some(names) = row.key(ELEMENT).and_then(|k| inherited.masks.get(&k)) else { continue };
        for name in names {
            row.cells.entry(name.clone()).or_insert_with(|| Cell::constant(diacraft_doc::Value::from_text("0")));
        }
    }
    let mut list: Vec<Arc<Row>> = rows.into_iter().map(Arc::new).collect();
    for key in inherited.keys.iter().filter(|k| !kept.contains(*k)) {
        let mut gone = Row { del: true, ..Row::default() };
        match key {
            RowKey::Ix(ix) => gone.ix = Some(*ix),
            RowKey::Name(name) => gone.name = Some(Arc::clone(name)),
        }
        list.push(Arc::new(gone));
    }
    list.sort_by_key(|row| (row.ix.is_none(), row.ix));
    let list = Rows::from(list);
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    // The section is made whole, with its rows, and put in the place of the one the shape held or after the others (`Counted` keeps the unknown nodes of the sheet where they stood).
    let mut counted = shape.counted_sheet();
    let held = counted.sheet.sections.iter().position(|s| s.name.as_str() == ELEMENT && s.ix.is_none());
    let section = match held.and_then(|i| counted.sheet.sections.get(i)) {
        Some(old) => Section {
            name: old.name.clone(),
            ix: old.ix,
            del: false,
            cells: old.cells.clone(),
            rows: list,
            raw: old.raw.clone(),
            passthrough: old.passthrough.clone(),
            ext: old.ext.clone(),
        },
        None => Section { rows: list, ..Section::new(ELEMENT, None) },
    };
    let section = Arc::new(section);
    match held.and_then(|i| counted.sheet.sections.get_mut(i)) {
        Some(slot) => *slot = section,
        None => {
            counted.push_section(section);
        }
    }
    Ok(())
}
