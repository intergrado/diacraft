//! The Operations menu of the Developer tab: Union, Combine, Fragment, Intersect, Subtract, Join, Trim, Offset and Fit Curve on the
//! shapes selected.
//!
//! **The window only sends commands.** Each entry runs one of the window's `operations.*` commands, which names the page the selection was made on and the
//! shapes selected, and is refused in words when that is not the page shown (`selection_target`). It sends the engine's `shape.union`, `shape.combine`,
//! `shape.fragment`, `shape.intersect`, `shape.subtract`, `shape.join`, `shape.trim`, `shape.offset` or `shape.fit_curve` (`docs/editing-commands.md`, "Shape operations",
//! "Join and Trim" and "Offset and Fit Curve"). **The last two take a number**: `operations.offset {distance}` and `operations.fitCurve {tolerance, flags?}` run with it, and the
//! menu's rows open their dialogs (`operations.offsetDialog`, `operations.fitCurveDialog`: `operations_dialog.rs`), whose OK runs them.
//! The engine makes the work one undo
//! step, and a refusal changes nothing and adds no step.
//!
//! **The order of `ids` is the order the selection is kept in** (project rule SHAPEOPS-PRIMARY), and the
//! first shape is the one a result takes its text and formatting from (and, for Subtract, the one that stays):
//!
//! * a **click selection keeps its click order**: a press on a shape selects it alone, and a Shift-click or a Ctrl-click adds a shape at the end
//!   (`EditState::selection` is a list, "the one chosen first leading"), so the shape clicked first is first;
//! * a **rubber band lists the shapes it takes in the page's drawing order**, back to front (`Scene::inside` goes through the page's shapes in order),
//!   so the backmost of them is first; a Shift-rubber-band adds them after what was selected. **Select All** is the same list;
//! * a selection made by `edit.select {ids}` is in the order of the `ids`.
//!
//! **An entry is enabled only when the engine says the operation can run on the selection**: [`DiacraftApp::shape_op_verdict`] asks
//! `diacraft_engine::draw::check_shape_ops`, which reads the shapes' geometry and so costs what reading them costs (13 ms on the worst input the engine's
//! tests make). **It is asked when the selection or the drawing changes and never for a frame**: the answer for each operation is kept for the drawing's
//! generation (**a file that is opened starts its revision at 1 again**, so the revision alone would find the old drawing's answers in the new one), its
//! revision, the page and the shapes selected, and an operation is asked about only when something draws or asks whether it can run (the Operations
//! menu's button asks for the first that is enabled, the menu for each of the nine). A refusal at the click (Intersect of shapes that do not meet passes
//! the check and is refused by its result) is the engine's own words in the status bar, and the drawing stays as it was.
//!
//! **Nothing stays selected** after any of the nine (the originals are gone, and a Subtract's first shape stays but is not selected; Offset keeps its originals and,
//! as the others, leaves nothing selected: SHAPEOPS-ORDER; Fit Curve changes the shapes in place and leaves nothing selected too), as the specification has it; Undo brings the
//! originals back and the window selects nothing then either (a selection is the window's).

use std::cell::RefCell;

use diacraft_engine::Result;
use diacraft_engine::draw::{Op, check_shape_ops};
use serde_json::json;

use crate::app::DiacraftApp;
use crate::format::SELECTION_ELSEWHERE;

/// The nine operations in the order of the menu.
pub const OPERATIONS: [Op; 9] = [Op::Union, Op::Combine, Op::Fragment, Op::Intersect, Op::Subtract, Op::Join, Op::Trim, Op::Offset, Op::FitCurve];

/// The window command that runs `op` on the shapes selected.
pub const fn command_of(op: Op) -> &'static str {
    match op {
        Op::Union => "operations.union",
        Op::Combine => "operations.combine",
        Op::Fragment => "operations.fragment",
        Op::Intersect => "operations.intersect",
        Op::Subtract => "operations.subtract",
        Op::Join => "operations.join",
        Op::Trim => "operations.trim",
        Op::Offset => "operations.offset",
        Op::FitCurve => "operations.fitCurve",
    }
}

/// The window command that opens the dialog of `op`, for the two that take a number.
pub const fn dialog_of(op: Op) -> Option<&'static str> {
    match op {
        Op::Offset => Some("operations.offsetDialog"),
        Op::FitCurve => Some("operations.fitCurveDialog"),
        Op::Union | Op::Combine | Op::Fragment | Op::Intersect | Op::Subtract | Op::Join | Op::Trim => None,
    }
}

/// The operation a window command runs, if it is one of the nine.
pub fn op_of_command(id: &str) -> Option<Op> {
    OPERATIONS.into_iter().find(|op| command_of(*op) == id)
}

/// The operation whose verdict says whether the window command `id` can run: the command that runs it, or the one that opens its dialog (a dialog opens when the operation
/// could run on the selection, and OK is then refused or not by the engine's words).
pub fn op_of_enabled_command(id: &str) -> Option<Op> {
    OPERATIONS.into_iter().find(|op| command_of(*op) == id || dialog_of(*op) == Some(id))
}

/// Where an operation's answer is kept in [`Held::answers`].
const fn slot(op: Op) -> usize {
    match op {
        Op::Union => 0,
        Op::Combine => 1,
        Op::Fragment => 2,
        Op::Intersect => 3,
        Op::Subtract => 4,
        Op::Join => 5,
        Op::Trim => 6,
        Op::Offset => 7,
        Op::FitCurve => 8,
    }
}

/// What the engine said an operation needs of the shapes: nothing, or in words what is wrong.
type Verdict = std::result::Result<(), String>;

/// The answers the engine gave for one drawing, revision, page and selection.
struct Held {
    generation: u64,
    revision: u64,
    page: u32,
    ids: Vec<u32>,
    /// One for each operation, asked for only when something wants it.
    answers: [Option<Verdict>; 9],
}

impl Held {
    fn is_for(&self, generation: u64, revision: u64, page: u32, ids: &[u32]) -> bool {
        (self.generation, self.revision, self.page) == (generation, revision, page) && self.ids == ids
    }
}

/// What the window remembers of whether the operations can run: the engine's answers for the last selection it was asked about.
#[derive(Default)]
pub struct OpsState {
    held: RefCell<Option<Held>>,
}

impl OpsState {
    /// The answer for `op` for this drawing, revision, page and selection: the one kept, or what `ask` says (once; it is kept).
    fn answer(&self, key: (u64, u64, u32), ids: &[u32], op: Op, ask: impl FnOnce() -> Verdict) -> Verdict {
        let (generation, revision, page) = key;
        if let Ok(held) = self.held.try_borrow()
            && let Some(held) = held.as_ref().filter(|h| h.is_for(generation, revision, page, ids))
            && let Some(known) = held.answers.get(slot(op)).and_then(Option::as_ref)
        {
            return known.clone();
        }
        let asked = ask();
        if let Ok(mut held) = self.held.try_borrow_mut() {
            let mut now = match held.take() {
                Some(h) if h.is_for(generation, revision, page, ids) => h,
                _ => Held { generation, revision, page, ids: ids.to_vec(), answers: Default::default() },
            };
            if let Some(place) = now.answers.get_mut(slot(op)) {
                *place = Some(asked.clone());
            }
            *held = Some(now);
        }
        asked
    }
}

impl DiacraftApp {
    /// Whether `op` can run on the shapes selected, and if not why in the engine's words. `Err("nothing is selected")` is the window's own.
    ///
    /// Kept for the drawing's generation and revision, the page and the shapes selected, so that the engine is asked when one of them changes and not
    /// every frame the ribbon is drawn.
    pub(crate) fn shape_op_verdict(&self, op: Op) -> Verdict {
        let Some(doc) = self.session.document() else { return Err("no document open".to_owned()) };
        let ids = self.ui.edit.selection();
        let Some(page) = self.ui.edit.selection_page().filter(|_| !ids.is_empty()) else { return Err("nothing is selected".to_owned()) };
        // The shapes selected are of the page the selection was made on, and a run is refused when another page is on view (`selection_target`): the entry
        // is quiet then, as the run would be refused. (A shape's `ID` is the page's own, so another page may hold shapes of the same numbers.)
        if self.ui.page.is_some_and(|shown| shown != page) {
            return Err(SELECTION_ELSEWHERE.to_owned());
        }
        let key = (self.ui.generation, self.session.revision(), page);
        self.ui.shape_ops.answer(key, ids, op, || {
            crate::work::count(|w| w.shape_op_checks += 1);
            check_shape_ops(doc, page, op, ids)
        })
    }

    /// `operations.union` and the other six that take nothing: the engine's command on the shapes selected, in the order they are kept in, on the page they are selected on
    /// (`selection_target` refuses a selection that is not of the page on view). Nothing is selected after it.
    pub(crate) fn run_shape_op(&mut self, op: Op) -> Result<()> {
        self.run_shape_op_with(op, json!({}))
    }

    /// [`DiacraftApp::run_shape_op`] with the parameters of Offset (`distance`) or of Fit Curve (`tolerance`, `flags`), in the cells' own units: added to the shapes and the page.
    pub(crate) fn run_shape_op_with(&mut self, op: Op, args: serde_json::Value) -> Result<()> {
        let (page, ids) = self.selection_target(command_of(op))?;
        let mut params = args;
        params["ids"] = json!(ids);
        params["page"] = json!(page);
        self.execute(op.id(), &params)?;
        self.select(&[]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn each_window_command_names_its_operation_and_its_engine_command() {
        for op in OPERATIONS {
            assert_eq!(op_of_command(command_of(op)), Some(op));
            assert!(command_of(op).starts_with("operations."), "{}", command_of(op));
            assert!(op.id().starts_with("shape."), "{}", op.id());
        }
        assert_eq!(op_of_command("shape.union"), None, "the engine's own id is not the window's");
        assert_eq!(op_of_command("operations.join"), Some(Op::Join));
        assert_eq!(op_of_command("operations.trim"), Some(Op::Trim));
        assert_eq!(op_of_command("operations.offset"), Some(Op::Offset));
        assert_eq!(op_of_command("operations.fitCurve"), Some(Op::FitCurve));
        assert_eq!(op_of_command("shape.fit_curve"), None, "the engine's own id is not the window's");
        // The dialogs share the verdict of the operation they run, and are no operation's command.
        assert_eq!(op_of_command("operations.offsetDialog"), None);
        assert_eq!(op_of_enabled_command("operations.offsetDialog"), Some(Op::Offset));
        assert_eq!(op_of_enabled_command("operations.fitCurveDialog"), Some(Op::FitCurve));
        assert_eq!(op_of_enabled_command("operations.union"), Some(Op::Union));
        assert_eq!(op_of_enabled_command("window.zoomDialog"), None);
        assert_eq!(dialog_of(Op::Union), None);
        let mut slots: Vec<usize> = OPERATIONS.iter().map(|op| slot(*op)).collect();
        slots.sort_unstable();
        assert_eq!(slots, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn an_answer_is_kept_for_its_drawing_revision_page_and_selection_and_asked_again_for_any_other() {
        let state = OpsState::default();
        let asked = Cell::new(0);
        let ask = |verdict: Verdict| {
            asked.set(asked.get() + 1);
            verdict
        };
        let key = (3, 1, 1);
        assert_eq!(state.answer(key, &[1, 2], Op::Union, || ask(Ok(()))), Ok(()));
        assert_eq!(state.answer(key, &[1, 2], Op::Union, || ask(Err("changed".into()))), Ok(()), "kept");
        assert_eq!(asked.get(), 1);
        // Another operation on the same selection is its own question, and is kept beside the first.
        assert_eq!(state.answer(key, &[1, 2], Op::Subtract, || ask(Err("no".into()))), Err("no".to_owned()));
        assert_eq!(state.answer(key, &[1, 2], Op::Union, || ask(Err("changed".into()))), Ok(()), "the first is still there");
        assert_eq!(asked.get(), 2);
        // The revision, the page, the drawing's generation and the shapes (and their order) each make it a new question.
        for (other, ids) in [((3, 2, 1), [1, 2]), ((3, 1, 2), [1, 2]), ((4, 1, 1), [1, 2]), ((3, 1, 1), [2, 1]), ((3, 1, 1), [1, 3])] {
            let before = asked.get();
            assert_eq!(state.answer(other, &ids, Op::Union, || ask(Err("new".into()))), Err("new".to_owned()), "{other:?} {ids:?}");
            // **A new key's answer is kept in turn**: asked again it is not asked, and is the answer given (an entry that kept the new answers under the old key
            // would ask each time).
            assert_eq!(state.answer(other, &ids, Op::Union, || ask(Err("again".into()))), Err("new".to_owned()), "{other:?} {ids:?} again");
            assert_eq!(
                state.answer(other, &ids, Op::Union, || ask(Err("and again".into()))),
                Err("new".to_owned()),
                "{other:?} {ids:?} a third time"
            );
            assert_eq!(asked.get(), before + 1, "{other:?} {ids:?} was asked more than once");
        }
    }

    #[test]
    fn a_selection_gone_back_to_is_asked_again_and_never_answered_with_another_selections_answer() {
        let state = OpsState::default();
        let key = (1, 1, 1);
        assert_eq!(state.answer(key, &[1], Op::Union, || Err("one shape".into())), Err("one shape".to_owned()));
        assert_eq!(state.answer(key, &[1, 2], Op::Union, || Ok(())), Ok(()));
        assert_eq!(state.answer(key, &[1], Op::Union, || Err("one shape, asked again".into())), Err("one shape, asked again".to_owned()));
        assert_eq!(state.answer(key, &[1], Op::Union, || Ok(())), Err("one shape, asked again".to_owned()), "and kept");
    }
}
