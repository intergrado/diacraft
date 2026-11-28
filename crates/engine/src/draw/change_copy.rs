//! `ReplaceCopyCells`: the cells a swap carries over, named by the new master.
//!
//! **The form.** The new master's own `ReplaceCopyCells` holds a `DEPENDSON` call whose arguments each name one cell (every one of the 206 cells in the files is such a
//! call; one that holds none, or says `No Formula`, names nothing). It is read on the master only, so an old shape's own copy of the cell changes nothing, and a
//! master of several top-level shapes (a group) says nothing, as it says nothing of the locks.
//!
//! **What a swap does with it.** Each cell named is carried from the old shape to the result, **whatever section it belongs to, as a value or as a formula** (a formula
//! that names other cells is carried as it is), and **beats the protection locks and the three `ReplaceLock` cells**: the list is carried after the merge and after the
//! shape data, so it has the last word. Where the new master gives the result no such cell **only the value is copied** (no formula, and the row is
//! made). A cell the old shape holds only from its own master or a style is carried as the constant it has now, as shape data is (a cell with `F="Inh"` is
//! no cell of its own).
//!
//! **What the specification leaves open is ours.** An argument that is no cell reference (a number, a string, a function), a reference to a cell of another shape
//! (`Sheet.5!Width`, `ThePage!...`) and a reference to a cell the old shape lacks (no row is made for it) are skipped, and the swap goes on. **At most
//! [`MAX_COPY_CELLS`] references are read**, in the order the call lists them. A cell of `Geometry` or `Connection` is carried only where the new master has the row (a row of one
//! cell would draw nothing right).
//!
//! **What it costs.** The list is read once for a change, from a formula the model already holds; each old shape locates the references with the evaluator's own reading
//! ([`diacraft_calc::Locator`]) within a budget of [`LOCATE_STEPS`], and writes at most [`MAX_COPY_CELLS`] cells.

use std::sync::Arc;

use diacraft_calc::{Budget, EvalCtx, EvalOptions, Locator};
use diacraft_doc::{Cell, CellAddr, Document, FormulaSlot, Resolver, ShapeRef, SheetRef};
use diacraft_formula::{CellRef, Expr};

use super::change::kept_cell;
use super::{Problem, shape_mut};

/// The most references of a list that are read.
pub(super) const MAX_COPY_CELLS: usize = 256;

/// The steps of the evaluator one old shape's references may take to be located.
const LOCATE_STEPS: u64 = 100_000;

/// A cell to carry: where it goes, the cell as the old shape has it, and whether the new master gives the result the cell too.
pub(super) struct Carried {
    addr: CellAddr,
    cell: Cell,
    gives: bool,
}

/// The references of the `DEPENDSON` calls of the `ReplaceCopyCells` cell of the master shape `sheet`, up to [`MAX_COPY_CELLS`], in the order they are
/// listed. No such cell, `No Formula` and a formula with no such call give none.
pub(super) fn list_of(doc: &Document, sheet: &SheetRef) -> Vec<CellRef> {
    let res = Resolver::new(doc);
    let Some(found) = res.resolve(sheet, &CellAddr::direct("ReplaceCopyCells")) else { return Vec::new() };
    let Some(FormulaSlot::Local(formula)) = found.formula.cell.map(|c| &c.formula) else { return Vec::new() };
    let Ok(expr) = &formula.ast else { return Vec::new() };
    let mut list: Vec<CellRef> = Vec::new();
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Call(c) if c.is("DEPENDSON") && c.qualifiers.is_empty() => {
                for arg in c.args.iter().flatten() {
                    if let Expr::Ref(r) = arg {
                        if list.len() >= MAX_COPY_CELLS {
                            return list;
                        }
                        list.push((**r).clone());
                    }
                }
            }
            Expr::Call(c) => stack.extend(c.args.iter().rev().flatten()),
            Expr::Unary(_, x) | Expr::Percent(x) => stack.push(x),
            Expr::Binary(_, a, b) => {
                stack.push(b);
                stack.push(a);
            }
            _ => {}
        }
    }
    list
}

/// Whether the sheet gives the cell, itself or through its master or a style.
fn holds(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> bool {
    res.resolve(sheet, addr).is_some_and(|found| found.formula.cell.is_some() || found.value.cell.is_some())
}

/// The cells of `list` that the old shape `old` has, as it has them, read before anything is changed. `new_sheet` is the new master's shape, which says for each whether the
/// result is given the cell too.
pub(super) fn carried(doc: &Document, old: &ShapeRef, new_sheet: Option<&SheetRef>, list: &[CellRef]) -> Vec<Carried> {
    let Some(new_sheet) = new_sheet else { return Vec::new() };
    let old_sheet = SheetRef::Shape(old.clone());
    let res = Resolver::new(doc);
    let mut ctx = EvalCtx::new(EvalOptions::default(), Budget::with_steps(LOCATE_STEPS));
    let mut locator = Locator::new(doc, &mut ctx);
    let mut out: Vec<Carried> = Vec::new();
    for reference in list {
        // A reference to another shape's cell is no cell of this one; a name that finds no row on the old shape finds nothing.
        let Some((sheet, addr)) = locator.locate(&old_sheet, reference) else {
            if locator.exhausted() {
                break;
            }
            continue;
        };
        if sheet != old_sheet || out.iter().any(|c| c.addr == addr) {
            continue;
        }
        let Some(cell) = res.resolve(&old_sheet, &addr).and_then(|found| kept_cell(&found)) else { continue };
        let gives = holds(&res, new_sheet, &addr);
        // A new master owns a two-dimensional shape's outline and points: a cell of them is carried only into a row it has.
        let outline = addr.section.as_ref().is_some_and(|s| matches!(s.name.as_str(), "Geometry" | "Connection"));
        if outline && !gives {
            continue;
        }
        out.push(Carried { addr, cell, gives });
    }
    out
}

/// Gives the changed shape the cells the list named, after everything else is written: the cell as the old shape had it where the new master gives the result the cell
/// too, and its value alone where it does not.
pub(super) fn put(doc: &mut Document, page: u32, at: &ShapeRef, cells: &[Carried]) -> Result<(), Problem> {
    if cells.is_empty() {
        return Ok(());
    }
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let sheet = Arc::make_mut(&mut shape.sheet);
    for carried in cells {
        let cell = if carried.gives {
            carried.cell.clone()
        } else {
            Cell { value: carried.cell.value.clone(), unit: carried.cell.unit.clone(), ..Cell::default() }
        };
        sheet.set_cell(&carried.addr, cell)?;
    }
    // A row of the outline the shape made for a cell it carries holds the type of the new master's row: a row with no `T` is no segment to another program.
    let sheet = SheetRef::Shape(at.clone());
    for carried in cells {
        if let (Some(section), Some(row)) = (&carried.addr.section, &carried.addr.row) {
            doc.give_row_type(&sheet, section, row)?;
        }
    }
    Ok(())
}
