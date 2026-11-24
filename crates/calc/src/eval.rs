//! The evaluator: a formula's tree reduced to one value, against the document model.
//!
//! **Where it runs.** A formula runs on the *inheritor*: the sheet it was asked on, whichever sheet holds the
//! text (a master shape, a style), so `Width` in an inherited `LocPinX` is the instance's own. [`Evaluator`] holds the document, a
//! resolver of it and an [`EvalCtx`], and `Evaluator::evaluate` takes the cell it is to work out.
//!
//! **Operands.** [`OperandMode::Cached`] reads a referenced cell as the file saved it (what the scans of the sample files of the specs did, so each
//! cell is tested alone), and [`OperandMode::Live`] works the referenced cell out first, memoised, with cycles and depth bounded
//! (what a recalculation after an edit does). Both read a failing cell at its last good value.
//!
//! **Hostile input.** A formula and a document are as hostile as the file they came from. Every node and every cell read costs a step
//! of the [`Budget`], and a spent budget, a cycle and a chain deeper than [`MAX_NATIVE_DEPTH`] give [`LOOP_ERROR`] and are counted in
//! [`EvalStats`]. Native recursion follows the nesting of one formula (at most `diacraft_formula::MAX_NESTING`) and a left-leaning
//! chain `a+b+c+...` is walked in a loop; a chain of cells in Live mode is walked with an explicit stack (`cells::prewarm`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_doc::{CellAddr, Document, Owner, Rebind, Resolver, SectionKey, SheetRef};
use diacraft_formula::{BinaryOp, Call, Expr, Number, UnaryOp, currency_by_code};
use diacraft_units::{Quantity, Unit};

use crate::cells::{CellEval, CellKey};
use crate::ops;
use crate::registry::{self, Kind};
use crate::row_names::RowNames;
use crate::rules::{BAD_ARGUMENT_ERROR, LOOP_ERROR, MAX_NATIVE_DEPTH, UNITS_PER_STEP};
use crate::shapes::{PeerNames, ShapeIndex};
use crate::strnum::multidimensional;
use crate::value::{CellError, Rgb, Value};

/// How an operand that is another cell is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperandMode {
    /// As the file saved it. Each cell is then tested on its own, and nothing recurses across cells.
    Cached,
    /// By working the cell out, memoised: what a recalculation after an edit needs.
    Live,
}

/// The clock and zone the clock-reading functions use: inputs, so a test fixes them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock {
    /// The wall clock now, in the zone of the machine: no zone is attached. `None` where there is no clock (the browser build
    /// reads none here), and `NOW` is then unsupported.
    pub now: Option<jiff::civil::DateTime>,
    /// The machine's offset from UTC, in seconds, which the document dates are shifted by.
    pub utc_offset_seconds: i32,
}

/// What the machine says of the file: not in the file, so an input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileInfo {
    /// The name of the file opened, or `None` for a file never saved.
    pub name: Option<String>,
    /// The folder it was opened from, or `None` where there is none (the web build).
    pub directory: Option<String>,
}

/// Options of an evaluation.
#[derive(Clone, Debug)]
pub struct EvalOptions {
    pub operands: OperandMode,
    pub clock: Clock,
    pub file: FileInfo,
    /// The seed of `RAND` (the generator is ours).
    pub rand_seed: u64,
    /// The faces `TEXTWIDTH` and `TEXTHEIGHT` measure with: the machine's, given by the caller. `None` measures with the bundled faces, which are
    /// the same on every machine (`fn_measure.rs`).
    pub fonts: Option<crate::fn_measure::Fonts>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        EvalOptions { operands: OperandMode::Live, clock: Clock::default(), file: FileInfo::default(), rand_seed: 0x9E37_79B9_7F4A_7C15, fonts: None }
    }
}

/// How much work and memory an evaluation may use. A step is meant to stand for a bounded cost, and does wherever work is charged (M2
/// And the fix rounds of Task 6): a node of a formula, a cell read, each [`rules::UNITS_PER_STEP`](crate::rules::UNITS_PER_STEP)
/// bytes of text a function reads, copies, builds or parses, and each place walked, listed or copied (a shape, a master, a style, a tie, a
/// binding, an item of a list), charged before the work is done or, for a listing that is built once and kept (`names.rs`, `shapes.rs`),
/// as it is built. Past the steps or the bytes the run ends as `budget_exhausted` and no more is worked out.
///
/// **What the steps do not bound:** the write phase of [`commit`](crate::commit), which goes through the document model's `write_batch`, finds
/// each sheet and row by a scan, and so costs more for a larger sheet or more masters written to (it is bounded by the memory the writes
/// waiting hold); the oracle's one pass over the document and the resolver's own indexes (`doc::Resolver`: built once for a large section or
/// sheet, at the cost of its size); and building the document. Text that the file sets is shared and not copied where a function hands it on
/// (a title, a name, a property, a shape's data), so handing it on costs nothing and whatever reads it pays for its length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub steps: u64,
    /// The memory an evaluation may hold at once, in bytes as accounted: the memo of cells worked out, the walks and maps of the oracle,
    /// the writes a commit has not made. The accounting counts what grows with the document (text, keys), not every allocation.
    pub bytes: u64,
}

impl Budget {
    /// The default for a whole document: see [`rules::DEFAULT_STEPS`](crate::rules::DEFAULT_STEPS) for how it was derived.
    pub const DEFAULT_STEPS: u64 = crate::rules::DEFAULT_STEPS;
    /// The default memory: see [`rules::DEFAULT_BYTES`](crate::rules::DEFAULT_BYTES).
    pub const DEFAULT_BYTES: u64 = crate::rules::DEFAULT_BYTES;

    /// A budget of `steps` with the default memory.
    pub const fn with_steps(steps: u64) -> Budget {
        Budget { steps, bytes: Budget::DEFAULT_BYTES }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Budget { steps: Budget::DEFAULT_STEPS, bytes: Budget::DEFAULT_BYTES }
    }
}

/// What an evaluation did that a report may show: counts, no formula and no value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalStats {
    /// Steps taken.
    pub steps: u64,
    /// Cells met again while working themselves out: a cycle.
    pub cycles: u64,
    /// Times the native depth limit was reached.
    pub depth_limit_hits: u64,
    /// Whether the budget ran out, of steps or of memory. Everything after that gives [`LOOP_ERROR`].
    pub budget_exhausted: bool,
    /// Whether it was the memory that ran out (`budget_exhausted` is then set too).
    pub memory_exhausted: bool,
    /// The most memory held at once, as accounted.
    pub bytes_peak: u64,
    /// Theme answers that took a fallback the theme batch says to warn of: a scheme number no theme part holds, answered by the first part
    /// And an entry number past its list, answered by the root style's cell or the table.
    /// Counted, so that a report can say so.
    pub theme_fallbacks: u64,
    /// Formula cells worked out: each time a cell's formula was run (a cell read from a memo is not one). A count of work that does not depend on how many
    /// steps a formula costs, so that a test can say a chain of cells was worked out once and not once for each cell that reads it.
    pub cells_worked: u64,
}

/// The state of an evaluation that outlives one call: the budget left, the memo of cells worked out, and what happened.
#[derive(Debug)]
pub struct EvalCtx {
    pub opts: EvalOptions,
    steps_left: u64,
    bytes_cap: u64,
    bytes_held: u64,
    pub stats: EvalStats,
    pub(crate) memo: HashMap<CellKey, Arc<CellEval>>,
    /// The cells worked out under the oracle's second reading, with the file's first theme part selected (the narrowing):
    /// a memo of their own for the whole walk, swapped in for [`EvalCtx::memo`] while that reading runs, so that a chain of cells is worked out once under
    /// each reading and not once for each cell that reads it. It is held memory, accounted as the first memo is.
    pub(crate) theme_memo: HashMap<CellKey, Arc<CellEval>>,
    pub(crate) in_progress: HashSet<CellKey>,
    pub(crate) rand_state: u64,
    /// Set when a clock-reading function ran since it was last cleared: the oracle clears it before a cell and reads it after.
    pub(crate) clock_touched: bool,
    /// Set when a text measure (`TEXTWIDTH`, `TEXTHEIGHT`) was made in a stand-in face that is not the name's open metric twin, since it was last cleared:
    /// carried by a cell as `clock_touched` is, so the oracle can leave out the cell and the cells that read it (`fn_measure.rs`).
    pub(crate) stand_in_touched: bool,
    /// Times the native depth limit was met, counting the times a cell read from the memo carries with it: a cell's share of it (the rise
    /// while it was worked out) says whether anything it read was cut short, whether that was read fresh or found already worked out.
    pub(crate) depth_seen: u64,
    /// What each formula holds that an analysis wants, found once per formula: see [`crate::facts`].
    pub(crate) facts: HashMap<usize, Arc<crate::facts::Facts>>,
    /// A cell whose saved value and error are put right out while it is worked out, and no other read of it: the real-file salt (`salt.rs`).
    pub(crate) salt: Option<(CellKey, crate::salt::Salted)>,
    /// Set when `REF()` was worked out, or a reference to a cell its section can never hold found nothing, since it was last cleared: the
    /// formula being worked out can only fail there. A cell takes it for its own formula alone.
    pub(crate) must_fail_touched: bool,
    /// Set when a theme answered a shadow property on the no-shadow branch, where the effect entry holds no outer shadow, since it was last
    /// cleared. A cell takes it from the cells it reads too, as it takes the clock.
    pub(crate) no_shadow_touched: bool,
    /// Set while the oracle works a cell out a second time with the file's first theme part selected (the narrowing, a
    /// project rule): a scheme cell that gives 0 because the sheet holds none, or follows a page that holds none, reads as that part
    /// ([`crate::rules::FIRST_THEME_PART`]). See [`Evaluator::under_first_theme_part`].
    pub(crate) first_theme_part: bool,
}

/// The counters of an evaluation, kept to be put back: the salt's work changes none of what a report says of the walk before it.
pub(crate) struct Counters {
    steps_left: u64,
    bytes_held: u64,
    stats: EvalStats,
}

impl EvalCtx {
    pub fn new(opts: EvalOptions, budget: Budget) -> EvalCtx {
        let rand_state = opts.rand_seed | 1;
        EvalCtx {
            opts,
            steps_left: budget.steps,
            bytes_cap: budget.bytes,
            bytes_held: 0,
            stats: EvalStats::default(),
            memo: HashMap::new(),
            theme_memo: HashMap::new(),
            in_progress: HashSet::new(),
            rand_state,
            clock_touched: false,
            stand_in_touched: false,
            depth_seen: 0,
            facts: HashMap::new(),
            salt: None,
            must_fail_touched: false,
            no_shadow_touched: false,
            first_theme_part: false,
        }
    }

    /// The counters now, to be put back by [`EvalCtx::restore_counters`].
    pub(crate) fn counters(&self) -> Counters {
        Counters { steps_left: self.steps_left, bytes_held: self.bytes_held, stats: self.stats }
    }

    /// Puts the counters back as they were (the budget spent, the memory held, the statistics), so that work done in between is not in any
    /// figure of the report and cannot end the run as `budget_exhausted`.
    pub(crate) fn restore_counters(&mut self, c: Counters) {
        self.steps_left = c.steps_left;
        self.bytes_held = c.bytes_held;
        self.stats = c.stats;
    }

    /// Whether `at` is the cell the salt is put right out in.
    pub(crate) fn salts(&self, sheet: &SheetRef, addr: &CellAddr) -> bool {
        self.salt.as_ref().is_some_and(|(key, _)| key.0 == *sheet && key.1 == *addr)
    }

    /// Forgets every cell worked out. A caller that changed the document calls it, since the memo belongs to the document it was made on.
    pub fn invalidate(&mut self) {
        self.memo.clear();
        self.theme_memo.clear();
        self.in_progress.clear();
        self.facts.clear();
        self.release_bytes(self.bytes_held);
    }

    /// Steps left of the budget.
    pub fn steps_left(&self) -> u64 {
        self.steps_left
    }

    pub(crate) fn take_step(&mut self) -> bool {
        self.take_steps(1)
    }

    /// Takes `n` steps in one go, charged before the work they pay for: false, and the run is over, when fewer are left.
    pub(crate) fn take_steps(&mut self, n: u64) -> bool {
        if self.stats.budget_exhausted || n > self.steps_left {
            self.steps_left = 0;
            self.stats.budget_exhausted = true;
            return false;
        }
        self.steps_left -= n;
        self.stats.steps += n;
        true
    }

    /// Holds `n` bytes more, as accounted: false, and the run is over (`memory_exhausted`), when that passes the budget's memory.
    pub(crate) fn hold_bytes(&mut self, n: u64) -> bool {
        if self.stats.budget_exhausted {
            return false;
        }
        let held = self.bytes_held.saturating_add(n);
        if held > self.bytes_cap {
            self.stats.memory_exhausted = true;
            self.stats.budget_exhausted = true;
            self.steps_left = 0;
            return false;
        }
        self.bytes_held = held;
        self.stats.bytes_peak = self.stats.bytes_peak.max(held);
        true
    }

    /// Lets `n` bytes go.
    pub(crate) fn release_bytes(&mut self, n: u64) {
        self.bytes_held = self.bytes_held.saturating_sub(n);
    }

    /// The memory held now, as accounted.
    pub fn bytes_held(&self) -> u64 {
        self.bytes_held
    }

    /// The next number of `RAND`: xorshift64*, in `[0, 1)`.
    pub(crate) fn next_random(&mut self) -> f64 {
        let mut x = self.rand_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rand_state = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // 53 random bits make a double in [0, 1).
        (r >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Default for EvalCtx {
    fn default() -> Self {
        EvalCtx::new(EvalOptions::default(), Budget::default())
    }
}

/// Where a cell is: its sheet and its place on it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellAddress {
    pub sheet: SheetRef,
    pub addr: CellAddr,
}

impl CellAddress {
    pub fn new(sheet: SheetRef, addr: CellAddr) -> CellAddress {
        CellAddress { sheet, addr }
    }
}

/// The context one formula runs in.
#[derive(Clone, Debug)]
pub(crate) struct Env {
    /// The reference context: the sheet a call or reference is read on. The inheritor, or the sheet a
    /// qualifier named.
    pub sheet: SheetRef,
    /// Set when the formula was found on a master: how `Sheet.<id>` names the inheritor's own shapes.
    pub rebind: Option<Rebind>,
    /// How many strings read as formulas this one is inside (`rules::MAX_STRING_FORMULA_DEPTH`).
    pub string_depth: usize,
    /// How many `EVALCELL`, `EVALTEXT` and the like this one is inside (`rules::MAX_NESTED_EVALUATIONS`).
    pub nested: usize,
    /// The names an `EVALCELL` bound for `ARG`.
    pub bindings: Option<Arc<Vec<(String, Value)>>>,
    /// The cell whose formula runs, for the few functions that read their host (`THEMEVAL()` with no argument).
    pub cell: Option<CellAddr>,
}

impl Env {
    pub(crate) fn new(sheet: SheetRef, rebind: Option<Rebind>) -> Env {
        Env { sheet, rebind, string_depth: 0, nested: 0, bindings: None, cell: None }
    }

    /// The same context on another sheet: what a qualifier makes.
    pub(crate) fn on(&self, sheet: SheetRef) -> Env {
        Env { sheet, rebind: None, string_depth: self.string_depth, nested: self.nested, bindings: self.bindings.clone(), cell: self.cell.clone() }
    }
}

/// A call as a function sees it: the call, the context its arguments are read in, and the context the call itself is read on, which a
/// sheet qualifier before it sets.
pub(crate) struct Site<'a> {
    pub call: &'a Call,
    pub env: &'a Env,
    pub target: &'a Env,
}

impl Site<'_> {
    /// The number of argument slots, empty ones included.
    pub(crate) fn len(&self) -> usize {
        self.call.args.len()
    }

    /// The expression in slot `i`, or `None` for an empty slot or one past the end.
    pub(crate) fn arg(&self, i: usize) -> Option<&Expr> {
        self.call.args.get(i).and_then(Option::as_ref)
    }
}

/// The evaluator over a document.
pub struct Evaluator<'d, 'c> {
    pub(crate) doc: &'d Document,
    pub(crate) res: Resolver<'d>,
    pub(crate) ctx: &'c mut EvalCtx,
    pub(crate) owners: HashMap<Owner, ShapeIndex>,
    pub(crate) peers: HashMap<(Owner, Vec<u32>), PeerNames>,
    /// The document's pages, masters, styles and faces by name, each listed on first use (`names.rs`).
    pub(crate) names: crate::names::DocNames,
    /// The ties of each shape's `Relationships` cell, read once: `None` for a shape with no such cell.
    pub(crate) ties: HashMap<SheetRef, Option<Arc<crate::fn_cells::Ties>>>,
    /// The names of the rows of a section as a sheet sees them, listed when a reference to a row finds nothing under its exact spelling
    /// (`row_names.rs`).
    pub(crate) row_names: HashMap<(SheetRef, SectionKey), Rc<RowNames>>,
    /// The group whose scheme cells a shape reads in place of the page's, for a shape in a group (`fn_color.rs`).
    pub(crate) scheme_groups: HashMap<SheetRef, Option<SheetRef>>,
    depth: usize,
}

impl<'d, 'c> Evaluator<'d, 'c> {
    /// An evaluator of `doc`. It builds nothing until a read needs it, and keeps what it builds, so one evaluator should work out as
    /// many cells as the caller has.
    pub fn new(doc: &'d Document, ctx: &'c mut EvalCtx) -> Evaluator<'d, 'c> {
        Evaluator {
            doc,
            res: Resolver::new(doc),
            ctx,
            owners: HashMap::new(),
            peers: HashMap::new(),
            names: Default::default(),
            ties: HashMap::new(),
            row_names: HashMap::new(),
            scheme_groups: HashMap::new(),
            depth: 0,
        }
    }

    /// The options of the evaluation.
    pub fn options(&self) -> &EvalOptions {
        &self.ctx.opts
    }

    /// Forgets every cell worked out (the memo of a `Live` evaluation), as after a change to the document.
    pub fn invalidate_memo(&mut self) {
        self.ctx.invalidate();
        self.ties.clear();
        self.row_names.clear();
        self.scheme_groups.clear();
    }

    /// The statistics so far.
    pub fn stats(&self) -> EvalStats {
        self.ctx.stats
    }

    /// Runs `work` with the file's first theme part selected where a sheet reads no theme (the narrowing: the oracle's
    /// second reading of a colour under a scheme the file no longer selects). A scheme cell that gives 0 because the sheet holds none, or follows a page
    /// that holds none, reads as [`crate::rules::FIRST_THEME_PART`], which `THEMEVAL` takes as the first part; a cell the sheet holds as an explicit 0
    /// stays 0.
    ///
    /// **The two readings have a memo each, for the whole walk.** In the live mode the cells a cell reads are worked out first and remembered, and under
    /// the second reading they are worked out under it: [`EvalCtx::theme_memo`] is swapped in for the memo of the first reading while `work` runs, and is
    /// the same map from one cell to the next, so a chain of cells is worked out once under each reading. (A memo made afresh for each cell worked its
    /// whole chain out again for each: a chain of 400 cost 738,600 steps against 10,400 for the first reading alone.) The cells it holds are held
    /// memory, accounted as the first memo's are, and go with [`EvalCtx::invalidate`]. In the cached mode no cell is memoised and the swap changes nothing.
    ///
    /// **What the reading leaves as it was.** The first reading's memo and cells in progress. The theme fallbacks the reading took (a scheme number no
    /// part holds takes the first part: that is this reading's way of selecting it) are not counted. The flags a cell sets for the cell that reads it (the
    /// clock, a stand-in face, a formula that can only fail, a no-shadow branch) need no care: `compute_cell` takes them at the start of every cell and puts
    /// them back at the end, and nothing reads them between two cells. **The steps it spends are spent**: it is work of the walk, and a budget it ends is
    /// the run's.
    pub(crate) fn under_first_theme_part<R>(&mut self, work: impl FnOnce(&mut Self) -> R) -> R {
        let theme_memo = std::mem::take(&mut self.ctx.theme_memo);
        let first_memo = std::mem::replace(&mut self.ctx.memo, theme_memo);
        let in_progress = std::mem::take(&mut self.ctx.in_progress);
        let fallbacks = self.ctx.stats.theme_fallbacks;
        self.ctx.first_theme_part = true;
        let out = work(self);
        self.ctx.first_theme_part = false;
        self.ctx.theme_memo = std::mem::replace(&mut self.ctx.memo, first_memo);
        self.ctx.in_progress = in_progress;
        self.ctx.stats.theme_fallbacks = fallbacks;
        out
    }

    /// Costs `n` steps; false when the budget is spent.
    pub(crate) fn steps(&mut self, n: usize) -> bool {
        self.ctx.take_steps(n as u64)
    }

    /// Costs the work of `units` bytes or nodes at [`UNITS_PER_STEP`] to a step (and one for the call), before it is done.
    pub(crate) fn work(&mut self, units: usize) -> bool {
        self.ctx.take_steps((units / UNITS_PER_STEP) as u64 + 1)
    }

    fn enter(&mut self) -> bool {
        self.enter_n(1)
    }

    fn leave(&mut self) {
        self.leave_n(1);
    }

    /// Enters `n` levels of native depth at once: a cell read in place stacks the frames of the whole read (resolve, work out, evaluate),
    /// which weigh many times a node's, so it counts as [`CELL_HOP_DEPTH`] levels (M1: a chain of 170 cells read from inside a function
    /// overflowed 512 KiB when each counted as one).
    pub(crate) fn enter_n(&mut self, n: usize) -> bool {
        if self.depth.saturating_add(n) > MAX_NATIVE_DEPTH {
            self.ctx.stats.depth_limit_hits += 1;
            self.ctx.depth_seen += 1;
            return false;
        }
        self.depth += n;
        true
    }

    pub(crate) fn leave_n(&mut self, n: usize) {
        self.depth = self.depth.saturating_sub(n);
    }

    /// Works out one expression in a context.
    pub(crate) fn eval(&mut self, e: &Expr, env: &Env) -> Value {
        if !self.ctx.take_step() {
            return Value::Error(LOOP_ERROR);
        }
        if !self.enter() {
            return Value::Error(LOOP_ERROR);
        }
        let v = self.eval_node(e, env);
        self.leave();
        v
    }

    fn eval_node(&mut self, e: &Expr, env: &Env) -> Value {
        match e {
            Expr::Number(n) => literal_number(n),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Str(s) => Value::Text(Arc::clone(s)),
            Expr::Color { r, g, b } => Value::Color(Rgb { r: *r, g: *g, b: *b }),
            Expr::Currency { value, code } => match currency_by_code(code) {
                Some(c) => Value::Currency { amount: *value, currency: Some(c) },
                // A tree built by hand may hold any code (the parser reads only the 192).
                None => Value::Error(CellError::Value),
            },
            // `{GUID}` is the text it spells, braces included.
            Expr::Guid(s) => Value::Text(Arc::clone(s)),
            Expr::Ref(r) => self.eval_ref(r, env),
            Expr::Call(c) => self.eval_call(c, env),
            Expr::Unary(op, x) => {
                let v = self.eval(x, env);
                self.unary(*op, v, env)
            }
            Expr::Percent(x) => {
                // `a%%%`: a run of `%` is a loop, as the parser reads it.
                let mut count = 1usize;
                let mut inner: &Expr = x;
                while let Expr::Percent(y) = inner {
                    count = count.saturating_add(1);
                    inner = y;
                }
                let mut v = self.eval(inner, env);
                for _ in 0..count.min(4096) {
                    if !self.steps(1) {
                        return Value::Error(LOOP_ERROR);
                    }
                    v = self.percent(v, env);
                }
                v
            }
            Expr::Binary(op, a, b) => {
                // A left-leaning chain `a+b+c+...` is walked in a loop: it adds a level to the tree for each term, and a formula
                // may hold 512 of them.
                let mut rights: Vec<(BinaryOp, &Expr)> = Vec::new();
                let mut op = *op;
                let mut right: &Expr = b;
                let mut left: &Expr = a;
                loop {
                    rights.push((op, right));
                    match left {
                        Expr::Binary(op2, a2, b2) => {
                            op = *op2;
                            right = b2;
                            left = a2;
                        }
                        _ => break,
                    }
                }
                let mut acc = self.eval(left, env);
                while let Some((op, rhs)) = rights.pop() {
                    let r = self.eval(rhs, env);
                    acc = self.binary(op, acc, r, env, false);
                }
                acc
            }
        }
    }

    /// `+a` and `-a`: the operand by `vDouble`, in its unit.
    pub(crate) fn unary(&mut self, op: UnaryOp, v: Value, env: &Env) -> Value {
        if v.is_failure() {
            return v;
        }
        match self.read_double_arith(&v, env) {
            Ok(q) => ops::sign(q, op == UnaryOp::Minus),
            Err(e) => e,
        }
    }

    /// `a%`.
    pub(crate) fn percent(&mut self, v: Value, env: &Env) -> Value {
        if v.is_failure() {
            return v;
        }
        match self.read_double_arith(&v, env) {
            Ok(q) => ops::percent(q),
            Err(e) => e,
        }
    }

    /// A binary operator on two values. `call` is true for `_DIV(a, b)`.
    pub(crate) fn binary(&mut self, op: BinaryOp, a: Value, b: Value, env: &Env, call: bool) -> Value {
        // An operand that is already an error comes back unchanged, the leftmost first.
        if a.is_failure() {
            return a;
        }
        if b.is_failure() {
            return b;
        }
        // A colour plus a number stays that colour (`TextBkgnd`'s custom colour entered as a call plus
        // one, and `THEMERESTORE`'s 0).
        if let (BinaryOp::Add, Value::Color(_), Value::Number(_) | Value::Bool(_)) = (op, &a, &b) {
            return a;
        }
        // The number first, when it is 0: 1,950 cells (`FillBkgnd`, `FillForegnd`, `GradientStopColor`) hold calls of `SETATREF` worth 0 before a
        // colour and save that colour and no `E`. Only 0 is attested, so only 0 keeps the colour.
        if let (BinaryOp::Add, Value::Number(n), Value::Color(_)) = (op, &a, &b)
            && n.stored == 0.0
        {
            return b;
        }
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                let x = match self.read_operand(&a, env) {
                    Ok(x) => x,
                    Err(e) => return e,
                };
                let y = match self.read_operand(&b, env) {
                    Ok(y) => y,
                    Err(e) => return e,
                };
                match op {
                    BinaryOp::Add => ops::add_sub(x, y, false),
                    BinaryOp::Sub => ops::add_sub(x, y, true),
                    BinaryOp::Mul => ops::mul(x, y),
                    _ => ops::div(x, y, call),
                }
            }
            BinaryOp::Pow => {
                let x = match self.read_double_arith(&a, env) {
                    Ok(x) => x,
                    Err(e) => return e,
                };
                let y = match self.read_double_arith(&b, env) {
                    Ok(y) => y,
                    Err(e) => return e,
                };
                ops::pow(x, y)
            }
            BinaryOp::Cat => {
                let x = match self.read_text(&a) {
                    Ok(x) => x,
                    Err(e) => return e,
                };
                let y = match self.read_text(&b) {
                    Ok(y) => y,
                    Err(e) => return e,
                };
                if x.len().saturating_add(y.len()) > crate::rules::MAX_TEXT_BYTES {
                    return Value::Error(BAD_ARGUMENT_ERROR);
                }
                // Building the join costs its length.
                if !self.work(x.len() + y.len()) {
                    return Value::Error(LOOP_ERROR);
                }
                let mut s = String::with_capacity(x.len() + y.len());
                s.push_str(&x);
                s.push_str(&y);
                Value::Text(Arc::from(s))
            }
            _ => {
                // The comparisons: both sides by `vDouble`, a string that cannot convert counting as 0.
                let x = match self.read_double_lenient(&a, env) {
                    Ok(x) => x,
                    Err(e) => return e,
                };
                let y = match self.read_double_lenient(&b, env) {
                    Ok(y) => y,
                    Err(e) => return e,
                };
                Value::Bool(ops::compare(op, x.stored, y.stored))
            }
        }
    }

    /// A call: a qualifier, a name, and arguments. A name the evaluator does not have is [`Value::Unsupported`],
    /// and so is a reserved one.
    fn eval_call(&mut self, c: &Call, env: &Env) -> Value {
        let qualified = match self.qualified(&c.qualifiers, env) {
            Ok(t) => t,
            Err(v) => return v,
        };
        let target: &Env = qualified.as_ref().unwrap_or(env);
        self.eval_call_in(c, env, target)
    }

    /// A call read on `target` (the context its qualifiers made) with its arguments read in `env`, the formula's own.
    pub(crate) fn eval_call_in(&mut self, c: &Call, env: &Env, target: &Env) -> Value {
        let Some(def) = registry::lookup(&c.name) else {
            return Value::unsupported(&c.name.to_ascii_uppercase());
        };
        if c.args.len() < usize::from(def.min) || (def.max != crate::registry::MANY && c.args.len() > usize::from(def.max)) {
            return Value::Error(BAD_ARGUMENT_ERROR);
        }
        let site = Site { call: c, env, target };
        match def.kind {
            Kind::Lazy(f) => f(self, &site),
            Kind::Eager(f) => {
                let mut slots: Vec<Option<Value>> = Vec::with_capacity(c.args.len());
                for arg in &c.args {
                    slots.push(arg.as_ref().map(|e| self.eval(e, env)));
                }
                if !def.sees_failures
                    && let Some(failure) = slots.iter().flatten().find(|v| v.is_failure())
                {
                    return failure.clone();
                }
                f(self, &site, &slots)
            }
        }
    }
}

/// A number literal as a value: its stored number, unit and dimension.
pub(crate) fn literal_number(n: &Number) -> Value {
    match n.quantity() {
        Some(q) => match *n {
            // `3[cm,D]` is a number in the unit written, shown in the document's default for that measure: stored it is the same.
            Number::Implicit { value, unit, measure } => {
                let q = Quantity::from_value(value, unit);
                let shown_in = match measure {
                    diacraft_formula::ImplicitMeasure::Angle => Unit::AngleDefault,
                    diacraft_formula::ImplicitMeasure::Drawing => Unit::DrawingDefault,
                    diacraft_formula::ImplicitMeasure::Duration => Unit::DurationDefault,
                    diacraft_formula::ImplicitMeasure::Page => Unit::PageDefault,
                    diacraft_formula::ImplicitMeasure::Type => Unit::TypeDefault,
                };
                Value::Number(Quantity::with_dim(q.stored, shown_in, q.dim()))
            }
            _ => Value::Number(q),
        },
        None => match *n {
            // `3 IN^2`: a unit with a power, and `#DIM!` outside -128 to 127.
            Number::Measured { value, unit, power: Some(p) } => match i8::try_from(p) {
                Ok(p8) => match multidimensional(value, unit, p8) {
                    Some(q) => Value::Number(q),
                    None => Value::Error(CellError::Dim),
                },
                Err(_) => Value::Error(CellError::Dim),
            },
            _ => Value::Error(CellError::Value),
        },
    }
}
