//! Chains of cells, cycles and the walks around them, without recursion.
//!
//! In [`crate::OperandMode::Live`] a cell is worked out from the cells it reads. A chain of a hundred thousand of them, `A1` reading
//! `A2` reading `A3` and so on, would stack a hundred thousand native frames if each read worked its cell out in place, and a file
//! chooses the length. So before a cell is worked out, [`Evaluator::prewarm`] walks the cells it reads *by name* (a reference, in any
//! branch of the formula) with an explicit stack and works the deepest first, remembering each, so that the native evaluation that
//! follows finds what it reads already done. What the walk cannot see (a cell reached through a sheet-returning call, `EVALCELL` or a
//! string read as a formula) is worked out in place, one native level at a time, and those levels are bounded
//! ([`crate::rules::MAX_NATIVE_DEPTH`]). A cell met again while it is being worked out is a cycle: [`crate::rules::LOOP_ERROR`], counted.

use std::collections::HashSet;

use diacraft_doc::{CellAddr, FormulaSlot, SheetRef};

use crate::cells::{CellKey, is_action_cell};
use crate::eval::{Env, Evaluator};
use crate::rules::KEY_BYTES;

/// The most cells one walk holds on its stack. A project rule that bounds the memory of a walk; past it the walk stops and the cells
/// that are left are worked out in place, to the native depth limit.
const MAX_WALK: usize = 4_000_000;

enum Step {
    Enter(CellKey),
    Exit(CellKey),
}

impl Evaluator<'_, '_> {
    /// Works out, deepest first and with an explicit stack, every cell the cell at `sheet`, `addr` reads by name, remembering each.
    ///
    /// The stack and the set of cells being visited are held memory, accounted as they grow ([`crate::rules::KEY_BYTES`] an entry); when
    /// the memory budget is passed the walk stops and the run ends as `budget_exhausted`.
    pub(crate) fn prewarm(&mut self, sheet: &SheetRef, addr: &CellAddr) {
        let root: CellKey = (sheet.clone(), addr.clone());
        if self.ctx.memo.contains_key(&root) {
            return;
        }
        let mut held: u64 = 0;
        let mut stack = vec![Step::Enter(root.clone())];
        let mut visiting: HashSet<CellKey> = HashSet::new();
        let mut deps: Vec<CellKey> = Vec::new();
        'walk: while let Some(step) = stack.pop() {
            if !self.steps(1) || stack.len() > MAX_WALK {
                break;
            }
            match step {
                Step::Enter(key) => {
                    if self.ctx.memo.contains_key(&key) || visiting.contains(&key) {
                        continue;
                    }
                    if !self.ctx.hold_bytes(2 * KEY_BYTES) {
                        break;
                    }
                    held += 2 * KEY_BYTES;
                    visiting.insert(key.clone());
                    stack.push(Step::Exit(key.clone()));
                    deps.clear();
                    self.static_deps(&key, &mut deps);
                    for d in deps.drain(..) {
                        if !self.ctx.memo.contains_key(&d) && !visiting.contains(&d) {
                            if !self.ctx.hold_bytes(KEY_BYTES) {
                                break 'walk;
                            }
                            held += KEY_BYTES;
                            stack.push(Step::Enter(d));
                        }
                    }
                }
                Step::Exit(key) => {
                    visiting.remove(&key);
                    // The cell asked for is worked out by the caller, after the cells it reads.
                    if key != root {
                        let _ = self.read_cell(&key.0, &key.1);
                    }
                }
            }
        }
        self.ctx.release_bytes(held);
    }

    /// The cells a cell's formula names, in any branch: references with a path qualifier or none. A reference qualified by a call, and
    /// what a function reaches by itself, are not named here. Charged a step a reference, over the formula's facts, which are found once.
    pub(crate) fn static_deps(&mut self, key: &CellKey, out: &mut Vec<CellKey>) {
        if is_action_cell(&key.1) {
            return;
        }
        let Some(res) = self.res.resolve(&key.0, &key.1) else { return };
        let Some(FormulaSlot::Local(formula)) = res.formula.cell.map(|c| &c.formula) else { return };
        let Some(facts) = self.facts_of(formula) else { return };
        let env = Env::new(res.on.clone(), res.rebind.clone());
        for r in &facts.refs {
            if !self.steps(1) {
                return;
            }
            if let Ok((sheet, addr)) = self.locate(r, &env) {
                out.push((sheet, addr));
            }
        }
    }
}
