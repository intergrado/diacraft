//! Writing a cell: what a write does to the cell, by where it comes from.
//!
//! A write in `doc` carries its origin, not only a target and a value: the same write is made, kept or skipped by whether
//! the cell is guarded and by what asked for it. What is not here: the rule for when an evaluation adds an `Inh` cell (only where the
//! result differs from the source's), which is `calc`'s commit and chooses whether to write; and the `BOUND` and `SETATREF` cells, which
//! survive a write because of what their formulas say.

use std::sync::Arc;

use diacraft_formula::Formula;

use crate::DocError;
use crate::cell::{Cell, FormulaSlot};
use crate::document::Document;
use crate::resolve::{Carried, Resolver};
use crate::sheet::{CellAddr, Lookup, Sheet};
use crate::sheet_ref::{Owner, SheetRef};
use crate::value::Value;

/// What a write puts in a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum CellEdit {
    /// A formula of its own (it makes the formula local), with the value an evaluation gave it, if there is one;
    /// without one the cell keeps the value it had until the next evaluation.
    Formula { formula: Arc<Formula>, value: Option<Value> },
    /// A value and no formula: the cell becomes a local constant.
    Value(Value),
    /// Takes the local formula away: the cell goes back to the formula it inherits, or reads `No Formula` when there is
    /// nothing to inherit. Its value stays as the last result until an evaluation.
    ClearFormula,
    /// The result of an evaluation (`calc`'s commit): the cached `V`, and `E` if the evaluation failed, with the formula left exactly as
    /// it is. A cell that is not there is made with `F="Inh"`, so the formula it inherits is never copied down; whether to
    /// write at all, for a result equal to the source's, is the committer's rule. Use it with [`WriteOrigin::Evaluation`], which a guard
    /// does not stop.
    ///
    /// `unit` is the cell's `U` when the result's kind is one a unit carries (`DATE`, `CY`, `PNT`, `NURBS`, `POLYLINE`, `MULTIDIM`),
    /// so that a second evaluation reads the value back as the kind it is; `None` leaves the unit the cell has.
    Cached { value: Value, error: Option<Arc<str>>, unit: Option<Arc<str>> },
    /// A whole cell, as a reader or a builder makes one.
    Cell(Cell),
}

/// Where a write comes from (the table, and the evaluation that table does not list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOrigin {
    /// An action in the drawing window: a drag, a resize, a rotate, group and ungroup.
    Drawing,
    /// Applying a style: it removes the local formulas of the cells it covers.
    StyleApply,
    /// The automation `Formula`, `FormulaU`, `Result` and `ResultIU`.
    Automation,
    /// The automation `FormulaForce`, `FormulaForceU`, `ResultForce` and `ResultIUForce`.
    AutomationForce,
    /// `SETF` in a formula.
    Setf,
    /// A data refresh on a shape data cell.
    DataRefresh,
    /// An evaluation committing what it computed, with [`CellEdit::Cached`] and nothing else (any other edit under this origin is refused,
    /// [`DocError::EvaluationEdit`], since it would remove a guard): not an edit of the formula, so a guard does not stop it. `GUARD(x)` evaluates to `x`
    /// and a guarded formula still re-evaluates when something it names changes (`LocPinX` `GUARD(Width*0.47)`
    /// follows a write of 6 to `Width`, to 2.82). Without this origin a commit to a guarded cell would have to pose as a forced write.
    Evaluation,
}

impl WriteOrigin {
    /// Whether a guard lets this write through (the table: only the forced automation writes, `SETF`, and an evaluation).
    fn passes_a_guard(self) -> bool {
        matches!(self, WriteOrigin::AutomationForce | WriteOrigin::Setf | WriteOrigin::Evaluation)
    }
}

/// What a write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The cell was written.
    Written,
    /// The cell is guarded (its own formula, or one it inherits) and the write is of a kind a guard stops: nothing changed. An
    /// automation write is also to be reported to the caller as an error.
    KeptGuarded,
    /// A data refresh skipped a guarded cell.
    Skipped,
}

/// One write of a batch: [`Document::write_cells`], [`Document::write_batch`].
#[derive(Clone, Debug, PartialEq)]
pub struct CellWrite {
    pub addr: CellAddr,
    pub edit: CellEdit,
    pub origin: WriteOrigin,
}

impl Document {
    /// Writes the cell at `addr` of `sheet`, as `origin` asks (the table): a one-write [`Document::write_cells`].
    ///
    /// **What it costs.** What is on the way to the cell is copied (the document's lists of pages, masters and styles are a few chunks
    /// each, the page's list of chunks, one chunk, the shape and its sheet: independent of the size of the document, see
    /// `tests/it/sharing.rs`), and the write is judged by a [`Resolver`], which builds the lookups its walk needs: the page by `ID`
    /// always, the masters when the cell reaches the master step and the styles when it reaches a style. For a cell in a family on a
    /// document of 65,536 styles that is a map of them, built for this one write. Many writes belong in one
    /// [`Document::write_cells`] or [`Document::write_batch`], which build them once.
    pub fn write_cell(&mut self, sheet: &SheetRef, addr: &CellAddr, edit: CellEdit, origin: WriteOrigin) -> Result<WriteOutcome, DocError> {
        let write = CellWrite { addr: addr.clone(), edit, origin };
        let mut outcomes = self.write_cells(sheet, vec![write])?;
        outcomes.pop().ok_or(DocError::NoSuchSheet)
    }

    /// Writes several cells of one sheet: a [`Document::write_batch`] of one sheet.
    pub fn write_cells(&mut self, sheet: &SheetRef, writes: Vec<CellWrite>) -> Result<Vec<WriteOutcome>, DocError> {
        self.write_batch(writes.into_iter().map(|w| (sheet.clone(), w)).collect())
    }

    /// Writes cells of any sheets, one after the other, building the lookups once: what the commit of an evaluation uses, so that N writes
    /// do not build the lookups N times, whether they are to one sheet or to the members of one instance, which share a master. The
    /// outcomes are in the order of the writes.
    ///
    /// **A batch is the same writes one by one.** Each write is judged (is the cell guarded, is there anything for a clear to go back to)
    /// against the document as the writes before it left it, exactly as [`Document::write_cell`] would. What is built once is what no write
    /// of a cell changes (the maps of pages, masters, styles and shapes, [`Carried`]); what depends on the cells (the indexes of sections and
    /// rows, whether a style takes part) is built again for a write that is judged. A write that is not (an evaluation's, which a guard does
    /// not stop, and is never a clear) is not judged at all.
    ///
    /// All or nothing: if one write is refused (a bad name, a limit, a sheet that is not there, a deleted row, an evaluation that is not a
    /// cached result) the document is as it was and the error is the first.
    pub fn write_batch(&mut self, writes: Vec<(SheetRef, CellWrite)>) -> Result<Vec<WriteOutcome>, DocError> {
        // A document is a handful of shared lists, so keeping it to put back costs a handful of pointers.
        let backup = self.clone();
        let mut carried = Carried::default();
        let mut outcomes = Vec::with_capacity(writes.len());
        for (sheet, write) in writes {
            let judged = {
                let resolver = Resolver::resume(self, carried);
                let judged = judge(&resolver, &sheet, &write);
                carried = resolver.into_carried();
                judged
            };
            let done = judged.and_then(|(protected, inheritable)| {
                let target = self.sheet_mut(&sheet).ok_or(DocError::NoSuchSheet)?;
                apply(target, write, protected, inheritable)
            });
            match done {
                Ok(outcome) => outcomes.push(outcome),
                Err(e) => {
                    *self = backup;
                    return Err(e);
                }
            }
        }
        Ok(outcomes)
    }

    /// The sheet `at` names, to change: every `Arc` on the way is made unique first, so a copy of the document that shares them
    /// keeps what it had (`Arc::make_mut`). `None` for a sheet that is not there, and then nothing was copied.
    pub(crate) fn sheet_mut(&mut self, at: &SheetRef) -> Option<&mut Sheet> {
        self.sheet_of(at)?;
        match at {
            SheetRef::Document => Some(Arc::make_mut(&mut self.sheet)),
            SheetRef::Style(id) => {
                let found = self.styles.iter().position(|s| s.id == *id)?;
                let style = Arc::make_mut(self.styles.get_mut(found)?);
                Some(Arc::make_mut(&mut style.sheet))
            }
            SheetRef::Page(id) => {
                let found = self.pages.iter().position(|p| p.id == *id)?;
                let page = Arc::make_mut(self.pages.get_mut(found)?);
                Some(Arc::make_mut(&mut page.sheet))
            }
            SheetRef::Master(id) => {
                let found = self.masters.iter().position(|m| m.id == *id)?;
                let master = Arc::make_mut(self.masters.get_mut(found)?);
                Some(Arc::make_mut(&mut master.sheet))
            }
            SheetRef::Shape(at) => {
                let list = match at.owner {
                    Owner::Page(id) => {
                        let found = self.pages.iter().position(|p| p.id == id)?;
                        &mut Arc::make_mut(self.pages.get_mut(found)?).shapes
                    }
                    Owner::Master(id) => {
                        let found = self.masters.iter().position(|m| m.id == id)?;
                        &mut Arc::make_mut(self.masters.get_mut(found)?).shapes
                    }
                };
                let (first, rest) = at.path.split_first()?;
                let mut shape = Arc::make_mut(list.get_mut(usize::try_from(*first).ok()?)?);
                for step in rest {
                    shape = Arc::make_mut(shape.children.get_mut(usize::try_from(*step).ok()?)?);
                }
                Some(Arc::make_mut(&mut shape.sheet))
            }
        }
    }
}

/// What the document says now about one write: whether the cell is guarded against it, and whether there is anything for a clear to go
/// back to. A write of an evaluation that is not a cached result is refused: only that is an evaluation's (it keeps the formula as it is,
/// which is why a guard does not stop it; a value or a clear under that origin would remove the guard).
fn judge(resolver: &Resolver<'_>, sheet: &SheetRef, write: &CellWrite) -> Result<(bool, bool), DocError> {
    if write.origin == WriteOrigin::Evaluation && !matches!(write.edit, CellEdit::Cached { .. }) {
        return Err(DocError::EvaluationEdit);
    }
    if resolver.sheet(sheet).is_none() {
        return Err(DocError::NoSuchSheet);
    }
    let protected =
        !write.origin.passes_a_guard() && resolver.resolve(sheet, &write.addr).is_some_and(|r| r.formula.cell.is_some_and(Cell::is_guarded));
    let inheritable = matches!(write.edit, CellEdit::ClearFormula) && resolver.inherits(sheet, &write.addr);
    Ok((protected, inheritable))
}

/// One write of a batch, on the sheet it is for.
fn apply(target: &mut Sheet, write: CellWrite, protected: bool, inheritable: bool) -> Result<WriteOutcome, DocError> {
    let CellWrite { addr, edit, origin } = write;
    // A write into a deleted row or section is refused whatever the guard says: it would be lost.
    if let Some(what) = target.deleted_part(&addr) {
        return Err(DocError::Deleted(what));
    }
    if protected {
        return Ok(if origin == WriteOrigin::DataRefresh { WriteOutcome::Skipped } else { WriteOutcome::KeptGuarded });
    }
    let old = match target.lookup(&addr) {
        Lookup::Found(cell) => Some(cell.clone()),
        _ => None,
    };
    let new = match edit {
        CellEdit::Formula { formula, value } => {
            let mut cell = old.unwrap_or_default();
            cell.formula = FormulaSlot::Local(formula);
            cell.value = value.or(cell.value);
            cell.error = None;
            cell
        }
        CellEdit::Value(value) => {
            let mut cell = old.unwrap_or_default();
            cell.formula = FormulaSlot::Absent;
            cell.value = Some(value);
            cell.error = None;
            cell
        }
        CellEdit::ClearFormula => {
            let Some(mut cell) = old else {
                return Ok(WriteOutcome::Written);
            };
            cell.formula = if inheritable { FormulaSlot::Inherit } else { FormulaSlot::NoFormula };
            cell.error = None;
            cell
        }
        CellEdit::Cached { value, error, unit } => {
            let mut cell = old.unwrap_or_else(|| Cell::inherited(None));
            cell.value = Some(value);
            cell.error = error;
            if unit.is_some() {
                cell.unit = unit;
            }
            cell
        }
        CellEdit::Cell(cell) => cell,
    };
    target.set_cell(&addr, new)?;
    Ok(WriteOutcome::Written)
}
