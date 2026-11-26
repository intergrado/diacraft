//! Which cell a reference in a formula names: the evaluator's own reading of a reference, offered to a program that must know where a formula reads
//! without working the formula out.
//!
//! A command that numbers the rows of a section again has to tell, among every formula of a page, the references that name a row that moves from those that name
//! another. A reference names a cell by a path (`Scratch.A3`) and, often, by qualifiers before it (`Sheet.10!`, `ThePage!`, `TheDoc!`, `Pages[Site 2]!`, a peer's
//! name), and which cell that is depends on the sheet the formula is on and on the document. [`Locator::locate`] gives it as the evaluator finds it, with the
//! same spelling of a row's name in any letter case: **one reading of the language, not two**.
//!
//! **Cost.** A locator wraps one evaluator, which builds its lookups when a read first needs them (a part's shapes by `ID`, a level's names) and keeps them, so a
//! caller that locates thousands of references makes one locator and pays for each lookup once. Every lookup is charged to the budget of the context it was made
//! with, and a budget that runs out leaves every later reference none and [`Locator::exhausted`] true.

use diacraft_doc::{CellAddr, Document, SheetRef};
use diacraft_formula::{CellRef, Qualifier};

use crate::eval::{Env, EvalCtx, Evaluator};

/// Finds the cells that references name, on a document, within the budget of the context it is made with.
pub struct Locator<'d, 'c> {
    ev: Evaluator<'d, 'c>,
}

impl<'d, 'c> Locator<'d, 'c> {
    /// A locator over `doc`, charging its work to `ctx`.
    pub fn new(doc: &'d Document, ctx: &'c mut EvalCtx) -> Locator<'d, 'c> {
        Locator { ev: Evaluator::new(doc, ctx) }
    }

    /// The sheet and the cell a reference names when it is written in a formula **on `sheet`**, as the evaluator finds them. `None` for a reference that finds no
    /// sheet (`Sheet.99!Width` with no shape 99), one whose path names no cell, one with a call before it (`LISTSHEETREF()!Width`: the sheet is chosen when the
    /// formula runs, so no one cell is named), and any reference once the budget has run out. A sheet that is not in the document is not looked at: an unqualified
    /// reference is the cell of that sheet it names, there or not.
    pub fn locate(&mut self, sheet: &SheetRef, reference: &CellRef) -> Option<(SheetRef, CellAddr)> {
        if reference.qualifiers.iter().any(|q| matches!(q, Qualifier::Call { .. })) {
            return None;
        }
        let env = Env::new(sheet.clone(), None);
        self.ev.locate(reference, &env).ok()
    }

    /// Whether the budget has run out, of steps or of memory: what a `None` from [`Locator::locate`] may then mean.
    pub fn exhausted(&self) -> bool {
        self.ev.stats().budget_exhausted
    }
}
