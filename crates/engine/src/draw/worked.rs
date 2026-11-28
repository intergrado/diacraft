//! Cells that hold a formula and no saved value, read as the picture draws them, **for the readers that edit and pick** (a follow-up of public issue 14).
//!
//! A file may leave the value out beside a formula (`<Cell N="PinX" F="1+1"/>`), and the renderer draws such a cell by what its formula works out to
//! (`crates/render/src/read.rs`). The readers of the editing commands, of the pointer (`ui-egui`'s `Scene`), of the snap and of the group's text target read a cell's saved value,
//! which such a cell has none of: a shape pinned at `=1+1` was drawn at 2 inches and picked, moved, snapped and shown as if it sat at the origin. A [`Worked`] is what they read it
//! with: [`Worked::number_of`] gives the saved number of a cell that has one, and the answer of the formula for a cell that holds a formula and no value saved anywhere on the way
//! to it ([`diacraft_doc::Resolved::holds_unsaved_formula`]). Nothing is written to the document: the answer is read and not stored in a cell, so a save still writes no value for a
//! cell the file had none for.
//!
//! **A reader holds one, and it costs nothing until a cell needs it.** The evaluator that works the formulas out is made on the first cell that holds a formula and no value, and
//! **kept for the reader's life**: what it builds (the lists of the page's shapes, the names, the memo of the cells it has worked out) is built once, and not for each cell, which
//! on a page of thousands of shapes is the cost of a hang. The answers are kept per cell for the same reason, so a cell is worked out once however many times it is read (a corner
//! of a box is read through the poses of the shape and its groups, a score of reads of the same eight cells).
//!
//! **The steps are bounded for all the readers of one operation, not for each.** Every [`Worked`] takes the steps it works with from a [`WorkedBudget`]: the one of the scope it
//! was made in ([`WorkedBudget::enter`]; the session enters a new one for every command it runs, and the window's scene holds its own), or, made outside any
//! scope, one of its own. It takes them a part at a time ([`CHUNK`]), so a reader made while another is alive still finds some, and gives back what it did not spend when it ends.
//! **When they run out, a cell is read as it was before** (as saved, which is no value), the readers go on with what they have, and nothing is cut short or fails: that is the
//! fallback for a hostile file. A single formula that needs more than a part is given up on (it is read as saved, and not tried again by this reader); a reader that gives up on
//! [`MAX_GIVE_UPS`] cells stops working cells out. The evaluator's memory is capped ([`WORKED_BYTES`]), and an evaluator that fills it is let go of and made again, which empties its
//! memo, so that a page of 30,000 shapes read whole is worked out in pieces and not held whole.
//!
//! **One rule for a formula that cannot be worked out**. A formula that gives no value (an error, a function the evaluator lacks, steps that ran out, a formula that
//! needed more than a part of them) is no [`Answer`], and every reader reads such a cell by [`Worked::unanswered`]: as a cell that holds no value anywhere on the way, which is the root
//! style's value for a shape's cell that the root style holds ([`diacraft_doc::Resolver::root_standin`], the function the renderer's reading and the formatting controls call as well),
//! and the reader's own default for any other. The renderer read such a cell that way and these readers as saved (0), so a cell could be drawn one way and picked, moved or reported
//! another; a test holds the two to each other. A colour, a text and `#N/A` are answers ([`Answer::Colour`], [`Answer::Text`], [`Answer::Empty`]) and no failure: a reader that wants a number
//! finds none in them, and nothing stands in.
//!
//! **What a reader reads through it.** A number ([`Worked::number_of`]), a text ([`Worked::text_of`]), the cells of a row by name ([`RowView`], for shape data, hyperlinks, a connector's
//! route, a container's cells and a layer's) and the rows of a geometry section ([`Worked::geom_rows`], which takes a constructor's text as well). **A reader keeps at most
//! [`MAX_ANSWER_TEXT`] bytes of one answer's text and [`MAX_ANSWER_BYTES`] of text in all**: a text a formula gives may be a megabyte, and a reader keeps its answers.
//!
//! **What is counted** ([`counts`]) is for the tests, which hold the work to a number: the formulas run, the answers found again, the cells left to their saved reading, and the
//! evaluators made.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, CellError, EvalCtx, EvalOptions, Evaluator, OperandMode, Value as Calc};
use diacraft_color::Rgba;
use diacraft_doc::{CellAddr, CellName, Document, EffectiveRow, Resolved, Resolver, RowKey, SectionKey, SheetRef, Value};
use diacraft_geom::GeomRow;
use diacraft_render::CellValue;

/// The steps that all the readers of one operation (a command, the building of a scene) may spend in all on cells that hold a formula and no saved value. **Measured**
/// (`timed_what_a_cell_costs_in_time_and_in_steps`): `=1+1` is 8 steps and 5 microseconds, so a step is some 0.6 microseconds, and `THEMEVAL()` is 25 steps. A page of 30,000 shapes whose
/// nine frame cells are all such formulas is 270,000 cells and 2.2 million steps, and this leaves room for all of them and for three times as much; spent whole, it is some 5 seconds
/// on a page built to spend it, where the 16 million that a first choice gave was 10.
pub const WORKED_STEPS: u64 = 8_000_000;

/// The steps a reader takes from its budget at a time, and the most one formula may spend: a part is some 30 milliseconds (a formula is some ten steps, and a chain of a thousand
/// cells is some eight thousand), and a reader made while another is alive finds the rest. **What a formula that is never answered costs each reader**: a part, and no more than
/// [`MAX_GIVE_UPS`] of them, which is some 0.1 second of a command that reads it again (a query of a drag, which reads the few shapes it looks at and makes a reader or three).
pub const CHUNK: u64 = 50_000;

/// How many cells a reader may give up on (a formula that needed more than a part) before it stops working cells out.
pub const MAX_GIVE_UPS: u32 = 4;

/// The memory, as the evaluator accounts it, that one reader's evaluator may hold in the memo of the cells it has worked out: about 160,000 cells. Past it the evaluator is let go of
/// and made again, and the cell it was working on is tried once more.
pub const WORKED_BYTES: u64 = 128 * 1024 * 1024;

/// The most answers one reader keeps (a cell's address and a number or a short text, some 250 bytes: 100 megabytes at most). Past it the reader works no more cells out and reads the rest
/// as saved, so the memory a reader holds is bounded by the cells and not by the steps (a formula of one node is a few steps, and the steps of a command are millions).
pub const MAX_ANSWERS: usize = 400_000;

/// The longest text of one answer a reader keeps, in bytes. A text a formula gives may be a megabyte (the evaluator's own cap), and a reader keeps its answers, so a text past this is
/// **no answer**: the cell is read by the one rule ([`Worked::unanswered`]), as a formula that gave none. The readers of text cut what they read well short of it (a link's address, a
/// field's value) or refuse it (a constructor's text is read only up to 64 KiB).
pub const MAX_ANSWER_TEXT: usize = 64 * 1024;

/// The most bytes of text answers one reader holds in all. Past it the reader works no more cells out, as it does past [`MAX_ANSWERS`]: the memory a reader holds is bounded by the text
/// it keeps and not by the cells (400,000 answers of 64 KiB would be 25 gigabytes).
pub const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;

/// The most evaluators one reader makes again for memory. A reader ends at [`MAX_ANSWERS`] answers, which is some three evaluators' worth of memo, so this is a second bound, for a
/// reader whose cells are each a long chain that fills an evaluator on its own.
const MAX_FLUSHES: u32 = 64;

/// What a cell that holds a formula and no saved value works out to. **A formula that gives none of these has no answer** ([`Worked::answer`] is `None`: an error, a function the
/// evaluator lacks, steps that ran out), and every reader reads that cell by the one rule ([`Worked::unanswered`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// A number (a length in inches, an angle in radians, a flag as 0 or 1, as the cells hold them). Never NaN or infinite.
    Number(f64),
    /// A text.
    Text(Arc<str>),
    /// A colour (`RGB(1,2,3)`): the reader of a colour cell takes it, and a reader that wants a number or a text finds none.
    Colour(u8, u8, u8),
    /// The formula answered that it holds **no value**: `#N/A`, which the theme gives for a stop past the end of its list. It is an answer, and
    /// reads as no value; nothing stands in for it, as the picture reads it.
    Empty,
}

/// What the readers of a thread have done, for the tests that hold the work to a number and not to a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Formulas run: the cells worked out.
    pub worked_out: u64,
    /// Answers found again in the table of a reader, which cost a lookup.
    pub found: u64,
    /// Cells left to their saved reading because the steps ran out or the formula needed more than a part.
    pub unanswered: u64,
    /// Evaluators made: one for a reader whatever the number of its cells, and another each time one is let go of for memory.
    pub evaluators: u64,
    /// Steps the formulas spent in all.
    pub steps: u64,
}

thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { worked_out: 0, found: 0, unanswered: 0, evaluators: 0, steps: 0 }) };
    static CURRENT: RefCell<Option<WorkedBudget>> = const { RefCell::new(None) };
}

/// What the calling thread's readers have done since [`reset_counts`].
pub fn counts() -> Counts {
    COUNTS.with(Cell::get)
}

/// Starts the calling thread's count again from nothing.
pub fn reset_counts() {
    COUNTS.with(|c| c.set(Counts::default()));
}

fn count(change: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut now = c.get();
        change(&mut now);
        c.set(now);
    });
}

/// A number of steps that readers share: the steps for the cells that hold a formula and no saved value that one operation may spend in all. Cloning it shares it.
#[derive(Clone, Debug)]
pub struct WorkedBudget(Rc<Cell<u64>>);

impl WorkedBudget {
    /// A budget of `steps`.
    pub fn new(steps: u64) -> WorkedBudget {
        WorkedBudget(Rc::new(Cell::new(steps)))
    }

    /// The steps left.
    pub fn left(&self) -> u64 {
        self.0.get()
    }

    /// Makes the readers made on this thread, until the guard is dropped, take their steps from this budget. A scope inside another puts the outer one back when it ends.
    pub fn enter(&self) -> WorkedScope {
        WorkedScope { previous: CURRENT.with(|current| current.replace(Some(self.clone()))) }
    }

    /// Takes up to `most` steps out of what is left, and says how many.
    fn take(&self, most: u64) -> u64 {
        let taken = self.0.get().min(most);
        self.0.set(self.0.get().saturating_sub(taken));
        taken
    }

    /// Puts steps back that a reader took and did not spend.
    fn give_back(&self, steps: u64) {
        self.0.set(self.0.get().saturating_add(steps));
    }
}

/// For as long as it lives, the readers made on this thread take their steps from the budget that was entered.
#[derive(Debug)]
pub struct WorkedScope {
    previous: Option<WorkedBudget>,
}

impl Drop for WorkedScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT.with(|current| *current.borrow_mut() = previous);
    }
}

/// The budget of the scope the calling thread is in, if it is in one.
fn current() -> Option<WorkedBudget> {
    CURRENT.with(|current| current.borrow().clone())
}

/// The steps a reader holds out of its budget: what it took and has not spent, given back when the reader ends. **This is a type of its own, with no lifetime, so that the
/// reader's end is not a use of the document it reads**: the commands read with a [`Reader`](super::Reader) and write to the same document when they are done with it, which a
/// `Drop` on a type that borrows the document would not let them do until the end of the scope.
struct Held {
    budget: WorkedBudget,
    steps: u64,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.budget.give_back(self.steps);
    }
}

/// The evaluator of a reader and what it has answered. Made when the first cell that holds a formula and no saved value is read.
struct State<'a> {
    ev: Option<Evaluator<'a, 'static>>,
    answers: HashMap<(SheetRef, CellAddr), Option<Answer>>,
    /// The budget the steps come from (the scope's when the reader was made in one, found at the first cell, else the reader's own) and what the evaluator holds of it.
    held: Option<Held>,
    give_ups: u32,
    flushes: u32,
    /// The bytes of text the answers hold ([`MAX_ANSWER_BYTES`]).
    text_bytes: usize,
    /// Whether the reader works no more cells out: its budget ran out, or it gave up on too many cells.
    out: bool,
}

/// A reader of the cells that hold a formula and no saved value, of one document. See the [module documentation](self).
pub struct Worked<'a> {
    doc: &'a Document,
    /// The steps taken from the budget at a time, and the most a formula may spend ([`CHUNK`]; a test gives fewer).
    part: u64,
    /// The memory the evaluator may hold ([`WORKED_BYTES`]; a test gives less).
    bytes: u64,
    /// The most answers the reader keeps ([`MAX_ANSWERS`]; a test gives fewer).
    answers: usize,
    state: RefCell<State<'a>>,
    /// The resolver the stand-in of a cell whose formula gave no value is found with ([`Worked::unanswered`]), made when it is first asked for.
    res: OnceCell<Resolver<'a>>,
}

impl std::fmt::Debug for Worked<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worked")
    }
}

/// The context the formulas are worked out in: `Live`, so that a cell a formula reads is worked out as well (it may have no saved value either), with the steps given to it a part
/// at a time and a memory of its own.
fn context(steps: u64, bytes: u64) -> EvalCtx {
    EvalCtx::new(EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() }, Budget { steps, bytes })
}

impl<'a> Worked<'a> {
    /// A reader of `doc`. It does nothing until a cell that holds a formula and no saved value is read.
    pub fn new(doc: &'a Document) -> Worked<'a> {
        Worked::with_limits(doc, CHUNK, WORKED_BYTES, MAX_ANSWERS)
    }

    /// A reader that takes `part` steps at a time, lets its evaluator hold `bytes` of memory and keeps `answers` answers at the most: [`Worked::new`] with limits a test can reach.
    pub(crate) fn with_limits(doc: &'a Document, part: u64, bytes: u64, answers: usize) -> Worked<'a> {
        Worked {
            doc,
            part,
            bytes,
            answers,
            state: RefCell::new(State { ev: None, answers: HashMap::new(), held: None, give_ups: 0, flushes: 0, text_bytes: 0, out: false }),
            res: OnceCell::new(),
        }
    }

    /// The number the cell at `addr` of `sheet` holds, found through `res`: its saved value, or for a cell that holds a formula and no value saved anywhere on the way to it what the
    /// formula works out to. `None` for a cell that is found nowhere, that holds no number, and one whose formula gives none.
    pub fn number_at(&self, res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<f64> {
        let found = res.resolve(sheet, addr)?;
        self.number_of(sheet, addr, &found)
    }

    /// [`Worked::number_at`] for a cell the caller has resolved: `found` is what `res.resolve(sheet, addr)` gave. A reader that has the cell of a row at hand (`EffectiveRow`) asks
    /// with the address of the row's cell (`CellAddr { section, row, cell }`).
    pub fn number_of(&self, sheet: &SheetRef, addr: &CellAddr, found: &Resolved<'_>) -> Option<f64> {
        if found.holds_unsaved_formula() {
            return match self.answer(sheet, addr) {
                Some(Answer::Number(n)) => Some(n),
                // A text, a colour and "no value" are no number, and nothing stands in for them: the picture reads a number from such a cell as the caller's default.
                Some(_) => None,
                None => self.unanswered(sheet, addr, found).and_then(Value::number),
            };
        }
        found.value.cell.and_then(|c| c.value.as_ref()).and_then(|v| v.number())
    }

    /// The text the cell at `addr` of `sheet` holds, as [`Worked::number_of`] reads a number: its saved text, or for a cell that holds a formula and no value saved anywhere on the way to
    /// it what the formula works out to (a number as the cell shows it, a colour as `#rrggbb`). `found` is what the resolver gave for the cell. `None` for a cell that holds no value.
    pub fn text_of(&self, sheet: &SheetRef, addr: &CellAddr, found: &Resolved<'_>) -> Option<String> {
        if found.holds_unsaved_formula() {
            return match self.answer(sheet, addr) {
                Some(Answer::Text(t)) => Some(t.to_string()),
                Some(Answer::Number(n)) => Some(n.to_string()),
                Some(Answer::Colour(r, g, b)) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
                Some(Answer::Empty) => None,
                None => self.unanswered(sheet, addr, found).map(|v| v.text().to_owned()),
            };
        }
        found.value.cell.and_then(|c| c.value.as_ref()).map(|v| v.text().to_owned())
    }

    /// **The one rule for a formula that cannot be worked out** (an error, a function the evaluator lacks, steps that ran out, a formula that needed more than a part of them), which is the
    /// picture's: the cell reads as a cell that holds no value anywhere on the way, and that is **the root style's value for a shape's cell that the root style holds**
    /// ([`Resolver::root_standin`], the very function the renderer's reading and the formatting controls call), and the reader's own default for any other. This gives the root's value, if
    /// there is one: `found` is what the resolver gave for the cell, which the caller has found to hold a formula and no saved value. The renderer read a formula that failed
    /// as the root's value and these readers as saved, so a cell could be drawn one way and picked, moved or reported another.
    pub fn unanswered(&self, sheet: &SheetRef, addr: &CellAddr, found: &Resolved<'_>) -> Option<&'a Value> {
        self.resolver().root_standin(sheet, addr, found)?.value.cell?.value.as_ref()
    }

    /// The resolver of this reader's document, made when it is first asked for: the one that finds the stand-in of a cell whose formula gave no value ([`Worked::unanswered`]), and that
    /// the readers that have no resolver of their own (a master's sizes, a page's scale) find their cells with.
    pub fn resolver(&self) -> &Resolver<'a> {
        self.res.get_or_init(|| Resolver::new(self.doc))
    }

    /// The number the cell `name`, directly under `sheet`, holds, found by inheritance and read as [`Worked::number_of`] reads it. `None` for a cell found nowhere.
    pub fn number_named(&self, sheet: &SheetRef, name: &str) -> Option<f64> {
        let addr = CellAddr::direct(name);
        let found = self.resolver().resolve(sheet, &addr)?;
        self.number_of(sheet, &addr, &found)
    }

    /// The rows of the section `key` of `sheet` as `geom` rows, as the renderer reads them: a number is saved, or for a cell that holds a formula and no saved value what the formula
    /// works out to (`X` of `Width` and no value is the width, and not 0). `rows` are the section's effective rows; the second number is how many were left out, as for
    /// [`diacraft_render::geom_rows`].
    pub fn geom_rows(&self, sheet: &SheetRef, key: &SectionKey, rows: &[EffectiveRow<'_>]) -> (Vec<GeomRow>, usize) {
        diacraft_render::geom_rows_worked(rows, |row, name, found| {
            if !found.holds_unsaved_formula() {
                return None;
            }
            let addr = CellAddr { section: Some(key.clone()), row: Some(row.key.clone()), cell: name.clone() };
            Some(match self.answer(sheet, &addr) {
                Some(Answer::Number(n)) => CellValue::Num(n),
                Some(Answer::Text(t)) => CellValue::Text(t),
                Some(Answer::Colour(r, g, b)) => CellValue::Color(Rgba::rgb(r, g, b)),
                Some(Answer::Empty) => CellValue::Missing,
                None => self.unanswered(sheet, &addr, found).and_then(Value::number).map_or(CellValue::Missing, CellValue::Num),
            })
        })
    }

    /// The row `row` of the section `section` of `sheet`, read through this reader ([`RowView`]): the cells of a row that a reader of shape data, hyperlinks, a connector's route or a
    /// layer reads by name, each as the picture reads it.
    pub fn row<'r>(&'r self, sheet: &'r SheetRef, section: &'r SectionKey, row: &'r EffectiveRow<'a>) -> RowView<'r, 'a> {
        RowView { worked: self, sheet, section, row }
    }

    /// What the formula of the cell at `addr` of `sheet` works out to, run on that sheet; the caller has found that the cell holds a formula and no saved value
    /// ([`Resolved::holds_unsaved_formula`]). `None` when the formula gives no number or text (an error, a function the evaluator lacks, a colour), when the steps ran out, and
    /// when the formula needed more than a part of them. Each cell is worked out once.
    pub fn answer(&self, sheet: &SheetRef, addr: &CellAddr) -> Option<Answer> {
        let key = (sheet.clone(), addr.clone());
        let mut state = self.state.borrow_mut();
        if let Some(known) = state.answers.get(&key) {
            count(|c| c.found = c.found.saturating_add(1));
            return known.clone();
        }
        if state.out {
            count(|c| c.unanswered = c.unanswered.saturating_add(1));
            return None;
        }
        let mut answer = self.work_out(&mut state, &key);
        if let Some(Answer::Text(text)) = &answer {
            if text.len() > MAX_ANSWER_TEXT {
                // Too long to keep: no answer, and the cell is read by the rule for a formula that gave none.
                count(|c| c.unanswered = c.unanswered.saturating_add(1));
                answer = None;
            } else {
                state.text_bytes = state.text_bytes.saturating_add(text.len());
                if state.text_bytes >= MAX_ANSWER_BYTES {
                    state.out = true;
                }
            }
        }
        state.answers.insert(key, answer.clone());
        // A table that is full is the end of the reader's work: what it holds is what it answers, and the rest is read as saved.
        if state.answers.len() >= self.answers {
            state.out = true;
        }
        answer
    }

    /// Runs the formula of `key`, in an evaluator with steps taken from the budget; lets go of the evaluator, and tries once more with a new one, if the memory it was allowed
    /// ran out, and gives up on the cell if the steps for one formula did.
    fn work_out(&self, state: &mut State<'a>, key: &(SheetRef, CellAddr)) -> Option<Answer> {
        let at = CellAddress::new(key.0.clone(), key.1.clone());
        let held = state.held.get_or_insert_with(|| Held { budget: current().unwrap_or_else(|| WorkedBudget::new(WORKED_STEPS)), steps: 0 });
        for _ in 0..2 {
            let (doc, bytes) = (self.doc, self.bytes);
            let ev = state.ev.get_or_insert_with(|| {
                count(|c| c.evaluators = c.evaluators.saturating_add(1));
                Evaluator::owning(doc, context(0, bytes))
            });
            // Steps for this formula: up to a part, from the budget.
            let have = ev.steps_left();
            if have < self.part {
                ev.grant_steps(held.budget.take(self.part - have));
            }
            held.steps = ev.steps_left();
            if held.steps == 0 {
                state.out = true;
                count(|c| c.unanswered = c.unanswered.saturating_add(1));
                return None;
            }
            let steps_before = ev.stats().steps;
            let worked = ev.evaluate_worked_out(&at);
            let stats = ev.stats();
            held.steps = ev.steps_left();
            count(|c| {
                c.worked_out = c.worked_out.saturating_add(1);
                c.steps = c.steps.saturating_add(stats.steps.saturating_sub(steps_before));
            });
            if !stats.budget_exhausted {
                return match worked {
                    Some(Calc::Number(q)) if q.stored.is_finite() => Some(Answer::Number(q.stored)),
                    Some(Calc::Bool(b)) => Some(Answer::Number(if b { 1.0 } else { 0.0 })),
                    Some(Calc::Text(t)) => Some(Answer::Text(t)),
                    Some(Calc::Color(c)) => Some(Answer::Colour(c.r, c.g, c.b)),
                    Some(Calc::Error(CellError::Na)) => Some(Answer::Empty),
                    _ => None,
                };
            }
            // The run ended: what the evaluator holds is no use (a budget that ran out stays out), and it is let go of with what it had left, which is nothing.
            state.ev = None;
            held.steps = 0;
            if stats.memory_exhausted && state.flushes < MAX_FLUSHES {
                state.flushes += 1;
                continue;
            }
            break;
        }
        // The budget is dry, or one formula needed more than a part, or the evaluator was let go of twice for one cell: the cell is read as it was before.
        count(|c| c.unanswered = c.unanswered.saturating_add(1));
        state.give_ups = state.give_ups.saturating_add(1);
        if held.budget.left() == 0 || state.give_ups >= MAX_GIVE_UPS || state.flushes >= MAX_FLUSHES {
            state.out = true;
        }
        None
    }
}

/// A row of a section as a reader of shape data, of hyperlinks, of a connector's route or of a container sees it: the cells of the effective row, each read by name as the picture reads it
/// ([`Worked::number_of`], [`Worked::text_of`]), so that a cell that holds a formula and no saved value is what its formula works out to. Made by [`Worked::row`].
pub struct RowView<'r, 'a> {
    worked: &'r Worked<'a>,
    sheet: &'r SheetRef,
    section: &'r SectionKey,
    row: &'r EffectiveRow<'a>,
}

impl<'r, 'a> RowView<'r, 'a> {
    /// The cell `name` of the row as the resolver found it, `None` when the row has none.
    pub fn found(&self, name: &str) -> Option<&'r Resolved<'a>> {
        self.row.cells.iter().find(|(n, _)| n.as_str() == name).map(|(_, r)| r)
    }

    /// The address of the row's cell `name`.
    fn addr(&self, name: &CellName) -> CellAddr {
        CellAddr { section: Some(self.section.clone()), row: Some(self.row.key.clone()), cell: name.clone() }
    }

    /// The number the cell holds: saved, or worked out for a cell that holds a formula and no saved value ([`Worked::number_of`]). `None` for a cell the row lacks.
    pub fn number(&self, name: &str) -> Option<f64> {
        let (cell, found) = self.row.cells.iter().find(|(n, _)| n.as_str() == name)?;
        self.worked.number_of(self.sheet, &self.addr(cell), found)
    }

    /// The text the cell holds: saved, or worked out ([`Worked::text_of`]). `None` for a cell the row lacks and one that holds no value.
    pub fn text(&self, name: &str) -> Option<String> {
        let (cell, found) = self.row.cells.iter().find(|(n, _)| n.as_str() == name)?;
        self.worked.text_of(self.sheet, &self.addr(cell), found)
    }

    /// The unit of the cell: the one on the cell that holds its value, and for a cell that holds no saved value the one on the cell that holds its formula.
    pub fn unit(&self, name: &str) -> Option<&'a str> {
        let found = self.found(name)?;
        match found.value.cell {
            Some(cell) => cell.unit.as_deref(),
            None => found.formula.cell.and_then(|c| c.unit.as_deref()),
        }
    }

    /// The key of the row.
    pub fn key(&self) -> &'r RowKey {
        &self.row.key
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{Cell as DocCell, Owner, RowKey, ShapeRef};
    use diacraft_geom::GeomRow;
    use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

    use super::*;

    fn user(name: &str) -> CellAddr {
        CellAddr::in_row("User", None, RowKey::name(name), "Value")
    }

    fn sheet(at: u32) -> SheetRef {
        SheetRef::Shape(ShapeRef::top(Owner::Page(1), at))
    }

    /// A page of `n` shapes whose `PinX` is the formula `1+1` and no value.
    fn page_of(n: u32) -> Document {
        let mut page = PageBuilder::new();
        for id in 1..=n {
            page = page.shape(ShapeBuilder::new(id).set(CellAddr::direct("PinX"), DocCell::with_formula("1+1", None)));
        }
        DocBuilder::new().page(1, page).build()
    }

    fn pin_x() -> CellAddr {
        CellAddr::direct("PinX")
    }

    #[test]
    fn a_cell_that_holds_a_formula_and_no_saved_value_is_what_the_formula_works_out_to_and_is_worked_out_once() {
        let doc = page_of(3);
        let res = Resolver::new(&doc);
        reset_counts();
        let worked = Worked::new(&doc);
        for _ in 0..5 {
            assert_eq!(worked.number_at(&res, &sheet(0), &pin_x()), Some(2.0));
        }
        assert_eq!(worked.number_at(&res, &sheet(1), &pin_x()), Some(2.0));
        let c = counts();
        assert_eq!((c.worked_out, c.found, c.evaluators), (2, 4, 1), "two cells run, the other reads found, and one evaluator for the page: {c:?}");
    }

    #[test]
    fn a_cell_with_a_saved_value_stands_as_saved_and_one_found_nowhere_is_no_number() {
        let doc = DocBuilder::new()
            .page(
                1,
                PageBuilder::new().shape(
                    ShapeBuilder::new(1)
                        .cell("PinX", "4")
                        .set(CellAddr::direct("PinY"), DocCell::with_formula("1+1", Some(diacraft_doc::Value::from_text("9")))),
                ),
            )
            .build();
        let res = Resolver::new(&doc);
        reset_counts();
        let worked = Worked::new(&doc);
        assert_eq!(worked.number_at(&res, &sheet(0), &pin_x()), Some(4.0));
        assert_eq!(worked.number_at(&res, &sheet(0), &CellAddr::direct("PinY")), Some(9.0), "a formula with a value is read as saved");
        assert_eq!(worked.number_at(&res, &sheet(0), &CellAddr::direct("Height")), None, "no cell");
        assert_eq!(counts(), Counts::default(), "nothing was worked out, and no evaluator was made");
    }

    #[test]
    fn a_formula_that_gives_no_number_is_no_answer_and_the_saved_reading_stands() {
        let doc = DocBuilder::new()
            .page(
                1,
                PageBuilder::new().shape(
                    ShapeBuilder::new(1)
                        .set(pin_x(), DocCell::with_formula("1/0", None))
                        .set(CellAddr::direct("PinY"), DocCell::with_formula("RGB(1,2,3)", None)),
                ),
            )
            .build();
        let res = Resolver::new(&doc);
        let worked = Worked::new(&doc);
        assert_eq!(worked.number_at(&res, &sheet(0), &pin_x()), None);
        assert_eq!(worked.number_at(&res, &sheet(0), &CellAddr::direct("PinY")), None, "a colour is no number");
    }

    #[test]
    fn a_cell_a_formula_reads_that_holds_no_saved_value_is_worked_out_first() {
        let doc = DocBuilder::new()
            .page(
                1,
                PageBuilder::new().shape(
                    ShapeBuilder::new(1).set(user("A"), DocCell::with_formula("2+3", None)).set(pin_x(), DocCell::with_formula("User.A*2", None)),
                ),
            )
            .build();
        let res = Resolver::new(&doc);
        assert_eq!(Worked::new(&doc).number_at(&res, &sheet(0), &pin_x()), Some(10.0));
    }

    #[test]
    #[ignore = "timed: what a cell costs in time, which the budget of steps and the cap of answers are chosen by; the counted tests hold the work"]
    fn timed_what_a_cell_costs_in_time_and_in_steps() {
        let doc = page_of(100_000);
        let res = Resolver::new(&doc);
        reset_counts();
        let worked = Worked::new(&doc);
        let started = std::time::Instant::now();
        let answered = (0..100_000).filter(|at| worked.number_at(&res, &sheet(*at), &pin_x()) == Some(2.0)).count();
        let took = started.elapsed();
        let c = counts();
        eprintln!(
            "{answered} cells in {took:?}: {:?} a cell, {} steps a cell, {:?} a step; the answers are capped at {MAX_ANSWERS}, the steps of a command at {WORKED_STEPS}",
            took / 100_000,
            c.steps / c.worked_out.max(1),
            took / u32::try_from(c.steps.max(1)).unwrap_or(u32::MAX)
        );
        assert_eq!(answered, 100_000);
    }

    #[test]
    fn the_steps_of_readers_made_in_one_scope_are_the_scopes_in_all_and_what_is_not_spent_is_given_back() {
        let doc = page_of(2_000);
        let res = Resolver::new(&doc);
        let budget = WorkedBudget::new(1_000_000);
        let _scope = budget.enter();
        {
            let worked = Worked::new(&doc);
            assert_eq!(worked.number_at(&res, &sheet(0), &pin_x()), Some(2.0));
            // It took a part and holds it: the budget is short of it.
            assert!(budget.left() < 1_000_000);
        }
        let spent = 1_000_000 - budget.left();
        assert!(spent > 0 && spent < 1_000, "a cell is some steps, and the rest of the part came back: {spent}");
    }

    #[test]
    fn a_scope_that_runs_dry_leaves_the_cells_to_their_saved_reading_for_every_reader_after() {
        let doc = page_of(4_000);
        let res = Resolver::new(&doc);
        // Steps for some of the cells only: one cell takes about as many steps as `spent` says.
        let one = {
            let budget = WorkedBudget::new(1_000_000);
            let _scope = budget.enter();
            let worked = Worked::new(&doc);
            assert_eq!(worked.number_at(&res, &sheet(0), &pin_x()), Some(2.0));
            drop(worked);
            1_000_000 - budget.left()
        };
        assert!(one > 0);
        let budget = WorkedBudget::new(one * 10);
        let _scope = budget.enter();
        reset_counts();
        let first = Worked::new(&doc);
        let answered = (0..4_000).filter(|at| first.number_at(&res, &sheet(*at), &pin_x()) == Some(2.0)).count();
        assert!((5..200).contains(&answered), "ten cells' worth of steps answered some cells and not the page: {answered}");
        assert!(counts().unanswered > 0, "the rest were left, and counted");
        drop(first);
        // A reader made after the budget ran dry works nothing out.
        let second = Worked::new(&doc);
        let before = counts().worked_out;
        assert_eq!(second.number_at(&res, &sheet(3_999), &pin_x()), None);
        assert_eq!(counts().worked_out, before, "no formula ran");
    }

    #[test]
    fn a_reader_made_while_another_is_alive_still_finds_steps() {
        let doc = page_of(10);
        let res = Resolver::new(&doc);
        let budget = WorkedBudget::new(WORKED_STEPS);
        let _scope = budget.enter();
        let outer = Worked::new(&doc);
        assert_eq!(outer.number_at(&res, &sheet(0), &pin_x()), Some(2.0));
        let inner = Worked::new(&doc);
        assert_eq!(inner.number_at(&res, &sheet(1), &pin_x()), Some(2.0), "the outer reader took a part, not all there was");
    }

    #[test]
    fn an_evaluator_that_fills_its_memory_is_let_go_of_and_made_again_and_every_cell_is_still_answered() {
        // Memory for a hundred cells or so only: the page's 2,000 are worked out in pieces, with a new evaluator for each, and every one is answered.
        let doc = page_of(2_000);
        let res = Resolver::new(&doc);
        let worked = Worked::with_limits(&doc, CHUNK, 100_000, MAX_ANSWERS);
        reset_counts();
        for at in 0..2_000 {
            assert_eq!(worked.number_at(&res, &sheet(at), &pin_x()), Some(2.0), "shape {at}");
        }
        let c = counts();
        assert!(c.evaluators > 1 && c.evaluators <= u64::from(MAX_FLUSHES) + 1, "{c:?}");
        assert_eq!(c.unanswered, 0, "{c:?}");
    }

    #[test]
    fn a_reader_whose_table_of_answers_is_full_works_no_more_out_and_what_it_holds_is_still_answered() {
        let doc = page_of(100);
        let res = Resolver::new(&doc);
        let worked = Worked::with_limits(&doc, CHUNK, WORKED_BYTES, 10);
        reset_counts();
        let answered = (0..100).filter(|at| worked.number_at(&res, &sheet(*at), &pin_x()) == Some(2.0)).count();
        assert_eq!(answered, 10, "ten answers kept, and the rest read as saved");
        let c = counts();
        assert_eq!((c.worked_out, c.unanswered), (10, 90), "{c:?}");
        assert_eq!(worked.number_at(&res, &sheet(3), &pin_x()), Some(2.0), "a cell it holds is found");
    }

    #[test]
    fn a_reader_made_in_no_scope_has_a_budget_of_its_own() {
        let doc = page_of(2);
        let res = Resolver::new(&doc);
        assert_eq!(Worked::new(&doc).number_at(&res, &sheet(0), &pin_x()), Some(2.0));
    }

    #[test]
    fn a_scope_inside_another_puts_the_outer_one_back_when_it_ends() {
        let (outer, inner) = (WorkedBudget::new(10), WorkedBudget::new(20));
        let _outer = outer.enter();
        assert_eq!(current().map(|b| b.left()), Some(10));
        {
            let _inner = inner.enter();
            assert_eq!(current().map(|b| b.left()), Some(20));
        }
        assert_eq!(current().map(|b| b.left()), Some(10));
        drop(_outer);
        assert!(current().is_none());
    }

    /// A formula that costs a good many steps: a chain of `n` cells each reading the one before.
    fn chain(n: u32) -> Document {
        let mut shape = ShapeBuilder::new(1).set(user("C0"), DocCell::with_formula("1", None));
        for i in 1..=n {
            shape = shape.set(user(&format!("C{i}")), DocCell::with_formula(&format!("User.C{}+1", i - 1), None));
        }
        DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
    }

    #[test]
    fn a_formula_that_needs_more_steps_than_a_part_is_given_up_on_once_and_the_reader_goes_on() {
        // A chain of 300 cells costs more than a part of 500 steps. It is no answer and is not tried again; the reader gives it up and answers other cells.
        let doc = chain(300);
        let res = Resolver::new(&doc);
        let worked = Worked::with_limits(&doc, 500, WORKED_BYTES, MAX_ANSWERS);
        reset_counts();
        let last = user("C300");
        assert_eq!(worked.number_at(&res, &sheet(0), &last), None, "too long for a part: read as saved, and so no number");
        let after = counts();
        assert_eq!(worked.number_at(&res, &sheet(0), &last), None);
        assert_eq!(counts().worked_out, after.worked_out, "and not worked out again");
        assert_eq!(worked.number_at(&res, &sheet(0), &user("C5")), Some(6.0), "a short cell is still answered");
    }

    #[test]
    fn a_reader_that_gives_up_on_too_many_cells_works_no_more_out() {
        let doc = chain(300);
        let res = Resolver::new(&doc);
        let worked = Worked::with_limits(&doc, 500, WORKED_BYTES, MAX_ANSWERS);
        for i in 0..u64::from(MAX_GIVE_UPS) {
            // A different long cell each time: the chain's own last cells, each past the part.
            assert_eq!(worked.number_at(&res, &sheet(0), &user(&format!("C{}", 300 - i))), None);
        }
        reset_counts();
        assert_eq!(worked.number_at(&res, &sheet(0), &user("C3")), None, "it has stopped");
        assert_eq!(counts().worked_out, 0);
    }

    /// A `PolylineTo` row at the end of a path of one `MoveTo`, whose `A` is `a`.
    fn polyline_doc(a: DocCell) -> Document {
        let at = |ix: u32, name: &str| CellAddr::in_row("Geometry", Some(0), RowKey::Ix(ix), name);
        let shape = ShapeBuilder::new(1)
            .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
            .set(at(1, "X"), DocCell::constant(diacraft_doc::Value::from_text("0")))
            .set(at(1, "Y"), DocCell::constant(diacraft_doc::Value::from_text("0")))
            .row_type("Geometry", Some(0), RowKey::Ix(2), "PolylineTo")
            .set(at(2, "X"), DocCell::with_formula("1+1", None))
            .set(at(2, "Y"), DocCell::constant(diacraft_doc::Value::from_text("1")))
            .set(at(2, "A"), a);
        DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
    }

    fn geom_of(doc: &Document) -> Vec<GeomRow> {
        let res = Resolver::new(doc);
        let key = SectionKey::new("Geometry", Some(0));
        let rows = res.effective_rows(&sheet(0), &key).unwrap();
        Worked::new(doc).geom_rows(&sheet(0), &key, &rows).0
    }

    #[test]
    fn a_constructor_that_is_a_formula_and_no_saved_value_is_the_polyline_the_formula_gives() {
        let text = "POLYLINE(0,0,0.5,0.5)";
        let saved = geom_of(&polyline_doc(DocCell::constant(diacraft_doc::Value::from_text(text))));
        let formula = geom_of(&polyline_doc(DocCell::with_formula(&format!("\"{text}\""), None)));
        assert!(matches!(saved.last(), Some(GeomRow::PolylineTo { polyline: Some(_), .. })), "the control reads a polyline: {saved:?}");
        assert_eq!(formula, saved, "a formula that gives the text is the text, and the `X` that is a formula is worked out too");
        // A formula that gives no text is no polyline, and nothing else changes.
        let failed = geom_of(&polyline_doc(DocCell::with_formula("1/0", None)));
        assert!(matches!(failed.last(), Some(GeomRow::PolylineTo { polyline: None, .. })), "{failed:?}");
    }

    #[test]
    fn a_text_longer_than_the_most_an_answer_keeps_is_no_answer_and_is_read_by_the_rule_for_a_formula_that_gave_none() {
        let long = "a".repeat(MAX_ANSWER_TEXT + 1);
        let fits = "a".repeat(MAX_ANSWER_TEXT);
        let doc = DocBuilder::new()
            .page(
                1,
                PageBuilder::new().shape(
                    ShapeBuilder::new(1)
                        .set(user("Long"), DocCell::with_formula(&format!("\"{long}\""), None))
                        .set(user("Fits"), DocCell::with_formula(&format!("\"{fits}\""), None)),
                ),
            )
            .build();
        let res = Resolver::new(&doc);
        let worked = Worked::new(&doc);
        let found = |name: &str| res.resolve(&sheet(0), &user(name)).unwrap();
        assert_eq!(worked.text_of(&sheet(0), &user("Fits"), &found("Fits")).map(|t| t.len()), Some(MAX_ANSWER_TEXT), "the most is kept");
        assert_eq!(worked.text_of(&sheet(0), &user("Long"), &found("Long")), None, "one byte more is no answer");
    }

    #[test]
    fn the_text_a_reader_keeps_is_bounded_in_all_and_past_it_the_rest_is_read_as_saved() {
        // 300 cells of 60 KB each is 18 MB of answers against the 16 MB a reader holds: it stops, and says so.
        let text = format!("\"{}\"", "a".repeat(60_000));
        let mut page = PageBuilder::new();
        for id in 1..=300 {
            page = page.shape(ShapeBuilder::new(id).set(user("T"), DocCell::with_formula(&text, None)));
        }
        let doc = DocBuilder::new().page(1, page).build();
        let res = Resolver::new(&doc);
        let worked = Worked::new(&doc);
        reset_counts();
        let answered = (0..300)
            .filter(|at| {
                let found = res.resolve(&sheet(*at), &user("T")).unwrap();
                worked.text_of(&sheet(*at), &user("T"), &found).is_some()
            })
            .count();
        assert!(answered * 60_000 < MAX_ANSWER_BYTES + 60_000 * 2, "at most the bytes a reader holds were kept: {answered} answers");
        assert!(answered > 100 && answered < 300, "some were answered and the rest were not: {answered}");
        assert!(counts().unanswered > 0);
    }
}
