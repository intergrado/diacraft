//! The oracle: every formula cell worked out and scored against the value the reference app saved (`V`) and the error it saved (`E`).
//!
//! A viewer draws saved values; this shows how far they can be trusted. [`recalc_all`] works out each formula cell of a document, the
//! cells of its page sheets, master sheets, style sheets and every shape (nested ones too), and compares the result with the cell's
//! saved `V` and `E` (`E` as well as `V`, since some functions change no value). It counts what agreed, lists what
//! differed, and counts the names of functions it does not have. The report holds names, counts and values, never a formula's text
//! (the file hash, category, score, cell and function names, error class and expected and actual values), and the per-file and
//! per-source scores of a run over many files are sums of reports.
//!
//! **This is the one comparer**. The harness that reads a file and prints a report (`diacraft-cli oracle`) joins the reader to
//! this and decides nothing about what agrees or what is left out; `compare.rs` is the comparison, and the reasons below are the whole list.
//!
//! **Every cell of the document is in exactly one place of the report.** A formula cell (`cells`) is agreed, differed (listed, or counted in
//! `differed_unlisted`), one of the `skipped` reasons, `unsupported` (by function name), or `unreached` (the budget ran out first). A cell
//! with no formula to work out is in `not_formulas` (a constant, `No Formula`, or a cell of a deleted section or row), so that
//! `cells_in_document` is the number of cells a walk of the document meets, and a test checks it against such a walk. A cell is *scored*,
//! agreed or differed, only if it is a formula that was worked out and a saved value or error stands to compare: an `Inh` cell whose chain
//! holds no formula is compared with nothing (it was once compared with itself), and a result that is unknown is never counted as agreed.
//!
//! **What is left out, and counted.** Action cells, a formula that does not parse (its saved value stands), a cell that
//! reads the clock or the machine, directly or through cells and text it reads (`NOW`, `RAND`, `DOCCREATION`, `DOCLASTSAVE`, `DOCLASTEDIT`, `DOCLASTPRINT`, `FILENAME`, `DIRECTORY`),
//! a cell with a saved
//! `Themed` or a `V` past the year 9999 **and no saved error** (with one, the error is compared), a colour under a theme the file
//! no longer selects **when the saved value is the reading under the file's first theme part and not the sheet's own** (the theme rule as a later spec narrowed it:
//! the cell is worked out twice; a `THEMEPROP` on a sheet that reads no theme is scored, which is 2 for
//! `Embellishment` there), a font
//! cell whose evaluation gives a number other than 0 (the saving machine's number), a shadow property a theme answers on
//! the branch where its effect entry holds no outer shadow, a cell that saves a value and no `E` whose own formula can only fail
//! and does, by a reached `REF()` or a cell its section can never hold (a `#REF!` from a row, shape, page or
//! master the file lacks stays scored), a cell
//! that reads an operand saved as `Themed` or as a date past 9999 (the operand is no value to work from), a cell that met the native depth
//! limit, a cell with nothing saved to compare, a
//! cell of a page, master or style whose `ID` an earlier one of the same kind already has (the document model addresses a sheet by its
//! `ID`, so the later one cannot be told from the first), and a cell whose function is not here. A skip loses a comparison and no more.
//! **The clock:** `recalc_all` has none, and a cell that reads one is skipped as a clock reader by name; the functions that read it need a
//! clock only to give a value, and a skipped cell is never compared with one.
//!
//! **The comparison** is in [`crate::compare`]: `E` first, then numbers within a relative 1E-9, infinity only with itself, a date within a
//! second, text exactly, a colour by its channels (one step apart where `MSOTINT` made it), a Boolean as the number it is, and
//! `ATAN2` with the two allowances. The report counts the agreed cells that agree only through an allowance
//! (`agreed_by_allowance`), so the score can be read with and without them. Beside the clock skip it says how many `DOC*()` cells fit the
//! quarter-hour check.
//!
//! **The real-file salt** ([`recalc_all_salted`], `salt.rs`): a seeded sample of the cells that agreed is given a wrong saved value, worked out
//! again and compared by the same comparer after the walk, on the budget it left, with the counters put back. `RecalcReport::real_salt` says how
//! many were caught; a calculator that hands back the saved values agrees with every real cell, and the one synthetic control cannot show it.
//!
//! **Who calculates is a parameter** ([`Calculator`], [`recalc_all_using`]): [`Native`] is the evaluator, and a harness or a test may stand
//! another in. That is how the harness shows its control catches a calculator that hands back the saved values (an echo): see
//! `diacraft-cli oracle`.
//!
//! **What it costs.** Each formula is walked once, by the address of its `Formula` ([`crate::facts`]); each clock walk is charged a step a
//! node and holds memory as accounted; a printed value is cut at [`MAX_PRINTED_VALUE`] and at most [`MAX_DIFFERENCES`] differences are
//! listed. Past the budget's steps or memory the run counts what is left as `unreached` and ends as `budget_exhausted`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, Document, FormulaSlot, MAX_SHAPE_DEPTH, Owner, RowKey, SectionKey, ShapeList, ShapeRef, Sheet, SheetRef, ValueKind, cell_info,
};
use diacraft_formula::Expr;

use crate::cells::{CellEval, CellKey, CellKind, FAR_YEAR, THEMED_MARKER, is_action_cell};
use crate::compare::{Comparison, Judged, agrees_strictly, judge};
use crate::eval::{Budget, CellAddress, Env, EvalCtx, EvalOptions, EvalStats, Evaluator, OperandMode};
use crate::facts::TextRead;
use crate::rules::{
    GEOMETRY_COMPARE_TOLERANCE, KEY_BYTES, MAX_DIFFERENCES, MAX_PRINTED_VALUE, PATH_COMPARE_TOLERANCE, TAINT_ENTRY_BYTES,
    TEXT_MEASURE_COMPARE_TOLERANCE,
};
use crate::salt::{MAX_MISSES_KEPT, RealSalt, SaltMiss, SaltPolicy, SaltRun};
use crate::store::{encode, iso_to_serial};
use crate::value::{CellError, Value};

/// How many distinct names of unsupported functions a report keeps; further names are counted under [`OTHER_UNSUPPORTED`].
const MAX_UNSUPPORTED_NAMES: usize = 10_000;
/// The most function names a difference lists (a formula can name thousands; the first of them tell what the cell is).
const MAX_DIFFERENCE_FUNCTIONS: usize = 64;
/// The name the unsupported functions past [`MAX_UNSUPPORTED_NAMES`] are counted under.
pub const OTHER_UNSUPPORTED: &str = "(other)";

/// How one cell differed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifferenceKind {
    /// The values differ, and the errors agree.
    Value,
    /// The errors differ (one side has an `E`, or they are not the same), and the values agree or are not compared.
    Error,
    /// Both.
    Both,
}

/// Where a sheet is, as numbers and fixed words: what a report names a sheet by. It holds no name a person made up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetPlace {
    /// The `DocumentSheet`.
    Document,
    /// A style sheet, by its `ID`.
    Style(u32),
    /// A page's sheet, by the page's `ID`.
    Page(u32),
    /// A master's sheet, by the master's `ID`.
    Master(u32),
    /// A shape of the page's or the master's part: its own `ID` (`None` when the shape could not be found again), and how deep it sits (1 for a
    /// shape directly in the part).
    Shape { owner: Owner, id: Option<u32>, depth: u32 },
}

impl SheetPlace {
    /// The place as a report words it: `document`, `style:<id>`, `page:<id>`, `master:<id>`, `page:<id>/shape:<ID>`, `master:<id>/shape:<ID>`.
    pub fn label(&self) -> String {
        match self {
            SheetPlace::Document => "document".to_owned(),
            SheetPlace::Style(id) => format!("style:{id}"),
            SheetPlace::Page(id) => format!("page:{id}"),
            SheetPlace::Master(id) => format!("master:{id}"),
            SheetPlace::Shape { owner, id, .. } => {
                let part = match owner {
                    Owner::Page(id) => format!("page:{id}"),
                    Owner::Master(id) => format!("master:{id}"),
                };
                match id {
                    Some(id) => format!("{part}/shape:{id}"),
                    None => part,
                }
            }
        }
    }
}

/// What kind of value a report line holds, so that a harness shows a number and a colour as they are and only the length of a text (a label
/// a person typed is content).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass {
    Number,
    Date,
    Bool,
    Colour,
    Currency,
    Point,
    Constructor,
    /// A string, with its length in characters.
    Text {
        chars: usize,
    },
}

/// The class of a value, or `None` for one that is no value (an error, an unsupported function, a sheet).
pub fn class_of(v: &Value) -> Option<ValueClass> {
    Some(match v {
        Value::Number(q) if q.unit == diacraft_units::Unit::Date => ValueClass::Date,
        Value::Number(_) => ValueClass::Number,
        Value::Bool(_) => ValueClass::Bool,
        Value::Color(_) => ValueClass::Colour,
        Value::Currency { .. } => ValueClass::Currency,
        Value::Point(_) => ValueClass::Point,
        Value::Constructor(_) => ValueClass::Constructor,
        Value::Text(t) => ValueClass::Text { chars: t.chars().count() },
        Value::Sheet(_) | Value::Error(_) | Value::Unsupported(_) => return None,
    })
}

/// A cell whose result is not what the file saved. No formula text: names and values only.
#[derive(Clone, Debug, PartialEq)]
pub struct Difference {
    /// Where the sheet is: `document`, `style:<id>`, `page:<id>`, `master:<id>`, and `/shape:<ID>` for a shape (its own `ID`).
    pub sheet: String,
    /// The cell: `Width`, `Geometry[0].IX=4.X`, `Prop.Cost.Value`. **Names in it can be a person's** (a row's, a section's): a harness that
    /// publishes a report names the cell from `address` and its own lists.
    pub cell: String,
    /// The sheet as numbers.
    pub place: SheetPlace,
    /// The cell's address on its sheet: section and its `IX`, row (by `IX` or by name) and the cell's name.
    pub address: CellAddr,
    /// Whether the cell holds `F` `Inh`, so the formula the result comes from is the master's or a style's.
    pub inherited: bool,
    /// The functions the cell's formula calls, in capitals, each once, in order.
    pub functions: Vec<String>,
    pub kind: DifferenceKind,
    /// What the file saved: `V`, then `U` when it has one, as a typed value prints, cut at [`MAX_PRINTED_VALUE`] characters; `None` when
    /// there is no `V`.
    pub saved: Option<String>,
    /// What kind of value `saved` is (`None` for `Themed`, a date past the year 9999 and no value).
    pub saved_class: Option<ValueClass>,
    /// The `E` the file saved.
    pub saved_error: Option<String>,
    /// What the evaluation gave, printed the same way.
    pub actual: Option<String>,
    /// What kind of value `actual` is.
    pub actual_class: Option<ValueClass>,
    /// The error it gave.
    pub actual_error: Option<String>,
}

/// How a function, or a kind of cell, scored: the cells whose formula calls it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FunctionScore {
    pub agreed: u64,
    pub differed: u64,
}

/// What was left out of the score, and why. Each cell counted here was not compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    /// Action cells: `Action`, `EventDblClick` and the rest.
    pub actions: u64,
    /// A formula that does not parse: the saved value stands.
    pub parse_failed: u64,
    /// A cell that reads the clock or the machine, directly or through what it reads.
    pub clock_readers: u64,
    /// A cell whose saved value is the theme marker `Themed` and that saved no error.
    pub themed: u64,
    /// A cell with no `V` and no `E` to compare.
    pub nothing_saved: u64,
    /// An `Inh` cell whose chain holds no formula (or only the default one a page prints), so there is nothing to work out: it was once
    /// scored by comparing its saved value with itself.
    pub inherited_no_formula: u64,
    /// A saved date past the year 9999 that saved no error.
    pub far_year: u64,
    /// A colour that `THEMEVAL` gives under a scheme the file no longer selects, **and whose saved value is the reading under the file's first theme
    /// part and not the sheet's own reading** (as narrowed it). The cells under the rule's condition that
    /// are scored are in `agreed` and `differed`; this is the number the harness leaves out, which no scan of the files could count.
    pub stale_theme_colour: u64,
    /// A font cell (`Font`, `AsianFont`, `ComplexScriptFont`) whose evaluation gives a number other than 0: the number is a font number of the
    /// machine that saved the file, which no part of the file records (a project rule of the oracle, as a cell that reads the clock is left out).
    pub machine_font_number: u64,
    /// A cell that measured text (`TEXTWIDTH`, `TEXTHEIGHT`), itself or in a cell it reads, in a stand-in face that is not the name's open metric
    /// twin: the widths differ from the original's, by 6 to 11 percent for the bundled face (a project rule
    /// of the oracle). The measure is still given, for a text box needs one; the score leaves the cell out and says so. A `TEXTHEIGHT` is
    /// only counted when the text wrapped, as a text that fits on its lines in a wider face fits in the narrower one.
    pub text_stand_in: u64,
    /// A shadow property that a theme answered on the no-shadow branch, where the effect entry holds no `a:outerShdw` (a project rule of the oracle, decided by the theme entry and not by the saved value).
    pub theme_no_shadow: u64,
    /// A cell that saves a value and no `E`, whose own formula can only fail and does: it reached a `REF()` whose error is its result, or a
    /// reference to a cell its section can never hold (a project rule of the oracle). A `#REF!` from a
    /// row, shape, page or master the file lacks is not this: it stays scored.
    pub formula_must_fail: u64,
    /// A cell that reads an operand saved as `Themed`: the operand is no value, so the cell cannot be worked out. It was counted as
    /// `unsupported_unlisted` by the harness, though it names no function (97,954 of that word's 97,961 cells in the first recalculation score).
    pub themed_operand: u64,
    /// A cell that reads an operand saved as a date past the year 9999: as [`Skipped::themed_operand`].
    pub far_year_operand: u64,
    /// A cell whose evaluation met the native depth limit, so its result is the limit's and not the formula's.
    pub depth_limit: u64,
    /// A cell on a page, master or style whose `ID` an earlier one of its kind has: the model finds a sheet by its `ID`, so the later one is
    /// not addressable and its cells would be scored against the first one's.
    pub ambiguous_sheet: u64,
}

impl Skipped {
    /// Every skipped cell.
    pub fn total(&self) -> u64 {
        self.actions
            + self.parse_failed
            + self.clock_readers
            + self.themed
            + self.nothing_saved
            + self.inherited_no_formula
            + self.far_year
            + self.stale_theme_colour
            + self.machine_font_number
            + self.text_stand_in
            + self.theme_no_shadow
            + self.formula_must_fail
            + self.themed_operand
            + self.far_year_operand
            + self.depth_limit
            + self.ambiguous_sheet
    }
}

/// The cells of a document that are not candidates, because there is no formula to work out. Counted so that every cell is in one place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotFormulas {
    /// A `V` and no `F`: a constant (lists the combinations).
    pub value_only: u64,
    /// `F="No Formula"`.
    pub no_formula: u64,
    /// A cell in a section or row marked `Del`, formula or not.
    pub deleted: u64,
    /// Of the cells above, those that carry an `E`: nothing here can check an error on a cell with no formula (`E` and `V` with no `F`
    /// in 409 files). Counted in addition, not instead.
    pub with_saved_error: u64,
}

impl NotFormulas {
    /// Every cell that is not a candidate.
    pub fn total(&self) -> u64 {
        self.value_only + self.no_formula + self.deleted
    }
}

/// The check beside the skip of `DOC*()` cells: `V` less the core property is a whole number of quarter
/// hours within 14 hours, and what is left over is at most 2 seconds, or for `DOCLASTEDIT` at most 180 seconds with `V` at or before the
/// property.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentDateChecks {
    pub fit: u64,
    pub not_fit: u64,
}

/// The score of one document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecalcReport {
    /// Cells whose value and error agree with what was saved.
    pub agreed: u64,
    /// Cells whose value or error does not: at most [`MAX_DIFFERENCES`] are listed.
    pub differed: Vec<Difference>,
    /// Differences past the list's cap, counted and not kept.
    pub differed_unlisted: u64,
    /// Cells that reach a function this evaluator does not have, by the name of the function in capitals. Never counted as agreed.
    pub unsupported: BTreeMap<String, u64>,
    pub skipped: Skipped,
    /// The cells that hold no formula: see [`NotFormulas`].
    pub not_formulas: NotFormulas,
    /// Per function: the cells whose formula calls it, agreed and differed.
    pub by_function: BTreeMap<String, FunctionScore>,
    /// Per kind of cell, by the section's name (empty for a cell directly on a sheet) and the cell's name, with no row: the cells scored.
    /// A name past the cap of distinct names is counted under `("(other)", "(other)")`.
    pub by_cell: BTreeMap<(String, String), FunctionScore>,
    /// Formula cells met, all of them: each is in exactly one of `agreed`, `differed`, `differed_unlisted`, `skipped`, `unsupported` or
    /// `unreached`.
    pub cells: u64,
    /// Formula cells not worked out because the budget ran out, those of every sheet after the cell it ran out on included.
    pub unreached: u64,
    /// Of the cells that agreed, those that agree only through an allowance: a size above 1 that the relative tolerance scales, the geometry
    /// batch's 1E-6, `MSOTINT`'s step or `ATAN2`'s two. With no allowance but an absolute 1E-9 they would have differed.
    pub agreed_by_allowance: u64,
    /// Of the cells that agreed, those that agree only because the pair of cells they are part of names the same ellipse as the file's (`C` and `D`
    /// of an elliptical arc's row, `_ELLIPSE_THETA` and `_ELLIPSE_ECC`: `compare.rs`), and not by raw value. These are counted in
    /// `agreed_by_allowance` too.
    pub agreed_by_curve: u64,
    /// The cells counted in `skipped.stale_theme_colour`, tallied by the functions their formulas call (each name once a cell, in capitals, the names past
    /// [`MAX_UNSUPPORTED_NAMES`] under [`OTHER_UNSUPPORTED`]): so that a jump in that count can be traced to a kind of formula. The rule can leave out a
    /// cell whose colour the first theme part gives and the sheet's no-theme reading gives alike, and the tally shows where.
    pub stale_theme_colour_by_function: BTreeMap<String, u64>,
    /// Whether the steps or the memory ran out.
    pub budget_exhausted: bool,
    /// Whether it was the memory (`budget_exhausted` is set too).
    pub memory_exhausted: bool,
    pub document_date_checks: DocumentDateChecks,
    /// What salting the real cells of the file found: empty unless the run asked for it ([`recalc_all_salted`]).
    pub real_salt: RealSalt,
    pub stats: EvalStats,
}

impl RecalcReport {
    /// Cells scored: agreed or differed.
    pub fn scored(&self) -> u64 {
        self.agreed + self.differed.len() as u64 + self.differed_unlisted
    }

    /// The cells counted unsupported, over every function name.
    pub fn unsupported_cells(&self) -> u64 {
        self.unsupported.values().sum()
    }

    /// Cells with a formula that were left out of the score for a reason (the sum of [`Skipped`]).
    pub fn left_out(&self) -> u64 {
        self.skipped.total()
    }

    /// Cells with a formula that could not be calculated: a function this evaluator does not have, or a budget that ran out first.
    pub fn not_calculated(&self) -> u64 {
        self.unsupported_cells() + self.unreached
    }

    /// Every formula cell, placed: scored, left out, or not calculated. It is [`RecalcReport::cells`] whenever the report is whole.
    pub fn accounted(&self) -> u64 {
        self.scored() + self.left_out() + self.not_calculated()
    }

    /// Every cell a walk of the document meets: the formula cells and the cells that hold no formula.
    pub fn cells_in_document(&self) -> u64 {
        self.cells + self.not_formulas.total()
    }

    /// Adds another report's counts to this one: the sum of a source's files. Differences are kept, to [`MAX_DIFFERENCES`].
    pub fn merge(&mut self, other: RecalcReport) {
        self.agreed += other.agreed;
        self.differed_unlisted += other.differed_unlisted;
        for d in other.differed {
            if self.differed.len() < MAX_DIFFERENCES {
                self.differed.push(d);
            } else {
                self.differed_unlisted += 1;
            }
        }
        for (name, n) in other.unsupported {
            *self.unsupported.entry(name).or_default() += n;
        }
        let (a, b) = (&mut self.skipped, other.skipped);
        a.actions += b.actions;
        a.parse_failed += b.parse_failed;
        a.clock_readers += b.clock_readers;
        a.themed += b.themed;
        a.nothing_saved += b.nothing_saved;
        a.inherited_no_formula += b.inherited_no_formula;
        a.far_year += b.far_year;
        a.stale_theme_colour += b.stale_theme_colour;
        a.machine_font_number += b.machine_font_number;
        a.text_stand_in += b.text_stand_in;
        a.theme_no_shadow += b.theme_no_shadow;
        a.formula_must_fail += b.formula_must_fail;
        a.themed_operand += b.themed_operand;
        a.far_year_operand += b.far_year_operand;
        a.depth_limit += b.depth_limit;
        a.ambiguous_sheet += b.ambiguous_sheet;
        let (a, b) = (&mut self.not_formulas, other.not_formulas);
        a.value_only += b.value_only;
        a.no_formula += b.no_formula;
        a.deleted += b.deleted;
        a.with_saved_error += b.with_saved_error;
        for (name, s) in other.by_function {
            let e = self.by_function.entry(name).or_default();
            e.agreed += s.agreed;
            e.differed += s.differed;
        }
        for (name, s) in other.by_cell {
            let e = self.by_cell.entry(name).or_default();
            e.agreed += s.agreed;
            e.differed += s.differed;
        }
        self.cells += other.cells;
        self.unreached += other.unreached;
        self.agreed_by_allowance += other.agreed_by_allowance;
        self.agreed_by_curve += other.agreed_by_curve;
        for (name, n) in other.stale_theme_colour_by_function {
            *self.stale_theme_colour_by_function.entry(name).or_default() += n;
        }
        self.budget_exhausted |= other.budget_exhausted;
        self.memory_exhausted |= other.memory_exhausted;
        self.document_date_checks.fit += other.document_date_checks.fit;
        self.document_date_checks.not_fit += other.document_date_checks.not_fit;
        self.stats.steps += other.stats.steps;
        self.stats.cycles += other.stats.cycles;
        self.stats.depth_limit_hits += other.stats.depth_limit_hits;
        self.stats.bytes_peak = self.stats.bytes_peak.max(other.stats.bytes_peak);
        self.real_salt.merge(other.real_salt);
    }
}

/// What works one formula cell out for [`recalc_all_using`]. [`Native`] is the evaluator. A harness stands another in to see what the comparer
/// makes of it: a calculator that handed back the saved values (an echo) agrees with every cell, so a harness checks a control document
/// whose saved values are wrong and fails the run when the control does not differ.
pub trait Calculator {
    /// The cell at `at` worked out: its result and the saved value and error, in a [`CellEval`].
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval>;
}

/// The evaluator of this crate: [`Evaluator::evaluate_cell`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Native;

impl Calculator for Native {
    fn calculate(&mut self, ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Arc<CellEval> {
        ev.evaluate_cell(at)
    }
}

/// Works out every formula cell of `doc` and scores it against the file: [`recalc_all_with`] reading each operand as the file saved it,
/// which is how the specs' own scans were run, so that each cell is tested alone.
pub fn recalc_all(doc: &Document, budget: &Budget) -> RecalcReport {
    recalc_all_with(doc, budget, EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() })
}

/// [`recalc_all`] with the options of the evaluation: `OperandMode::Live` works each operand out first, so a wrong function shows in
/// every cell that reads it.
pub fn recalc_all_with(doc: &Document, budget: &Budget, options: EvalOptions) -> RecalcReport {
    recalc_all_using(doc, budget, options, &mut Native)
}

/// [`recalc_all_with`] with the calculator given: the one comparer, over whatever calculates. No real-file salt is run: see
/// [`recalc_all_salted`], which every scored run of the harness uses.
pub fn recalc_all_using(doc: &Document, budget: &Budget, options: EvalOptions, calc: &mut dyn Calculator) -> RecalcReport {
    recalc_all_salted(doc, budget, options, calc, None)
}

/// [`recalc_all_using`], and with a [`SaltPolicy`] the real-file salt (`salt.rs`): after the walk, a seeded sample of the cells that agreed is
/// worked out again by `calc` with a wrong saved value put in, and compared by the same comparer. How many were caught, and which were not, is
/// `RecalcReport::real_salt`. The salt is worked out on the budget the walk left and puts the counters back: **it changes no figure of the
/// score**, and a file whose budget ran out is not salted past that point (`RealSalt::not_checked`).
pub fn recalc_all_salted(doc: &Document, budget: &Budget, options: EvalOptions, calc: &mut dyn Calculator, salt: Option<SaltPolicy>) -> RecalcReport {
    let mut ctx = EvalCtx::new(options, *budget);
    let mut report = RecalcReport::default();
    {
        let mut ev = Evaluator::new(doc, &mut ctx);
        let mut walker = Walker { salt: salt.map(SaltRun::new), ..Walker::default() };
        // Every sheet is visited, after the budget has run out too: a cell is then counted `unreached` and not worked out, so the report
        // accounts for all of them, and the walk of the document is linear.
        walk_sheets_checked(doc, &mut |sheet, data, addressable| {
            count_not_formulas(data, &mut report.not_formulas);
            for_each_formula_cell(data, |addr, inherited| {
                if addressable {
                    score_cell(&mut ev, calc, &mut walker, &mut report, sheet, addr, inherited);
                } else {
                    report.cells += 1;
                    report.skipped.ambiguous_sheet += 1;
                }
            });
        });
        if let Some(run) = walker.salt.take() {
            report.real_salt = work_out_the_salt(&mut ev, calc, run);
        }
    }
    report.stats = ctx.stats;
    report.budget_exhausted |= ctx.stats.budget_exhausted;
    report.memory_exhausted |= ctx.stats.memory_exhausted;
    report
}

/// Counts the cells of a sheet that hold no formula to work out: constants, `No Formula`, and every cell of a deleted section or row.
fn count_not_formulas(sheet: &Sheet, out: &mut NotFormulas) {
    let mut one = |cell: &Cell, deleted: bool| {
        let counted = if deleted {
            out.deleted += 1;
            true
        } else {
            match cell.formula {
                FormulaSlot::Absent => {
                    out.value_only += 1;
                    true
                }
                FormulaSlot::NoFormula => {
                    out.no_formula += 1;
                    true
                }
                FormulaSlot::Local(_) | FormulaSlot::Inherit => false,
            }
        };
        if counted && cell.error.is_some() {
            out.with_saved_error += 1;
        }
    };
    for cell in sheet.cells.values() {
        one(cell, false);
    }
    for section in &sheet.sections {
        for cell in section.cells.values() {
            one(cell, section.del);
        }
        for row in &section.rows {
            for cell in row.cells.values() {
                one(cell, section.del || row.del);
            }
        }
    }
}

/// Calls `f` with every sheet of the document and its data: the document's own, each style's, each page's and master's, and every shape of
/// every part, nested ones too, found with an explicit stack and one at a time (no list of them is built). Nothing here looks a sheet up.
pub(crate) fn walk_sheets(doc: &Document, f: &mut dyn FnMut(&SheetRef, &Sheet)) {
    walk_sheets_checked(doc, &mut |sheet, data, _| f(sheet, data));
}

/// As [`walk_sheets`], and `f` is told whether the sheet can be addressed. The model finds a style, a page or a master by its `ID` and takes
/// the first, so a later one of the same `ID` (and every shape of a later page or master) is **not addressable**: a cell read at its address
/// is the first one's, and scoring it would compare the first sheet's result with the later sheet's saved value.
pub(crate) fn walk_sheets_checked(doc: &Document, f: &mut dyn FnMut(&SheetRef, &Sheet, bool)) {
    f(&SheetRef::Document, &doc.sheet, true);
    let mut seen: HashSet<u32> = HashSet::new();
    for style in doc.styles.iter() {
        f(&SheetRef::Style(style.id), &style.sheet, seen.insert(style.id));
    }
    seen.clear();
    for page in doc.pages.iter() {
        let first = seen.insert(page.id);
        f(&SheetRef::Page(page.id), &page.sheet, first);
        walk_part(Owner::Page(page.id), &page.shapes, first, f);
    }
    seen.clear();
    for master in doc.masters.iter() {
        let first = seen.insert(master.id);
        f(&SheetRef::Master(master.id), &master.sheet, first);
        walk_part(Owner::Master(master.id), &master.shapes, first, f);
    }
}

/// Calls `f` with the sheet of one page and every shape of its part, and with nothing else: what an edit to that page can have changed.
/// A page that is not there gives nothing.
pub(crate) fn walk_page(doc: &Document, page: u32, f: &mut dyn FnMut(&SheetRef, &Sheet)) {
    if let Some(found) = doc.page(page) {
        f(&SheetRef::Page(found.id), &found.sheet);
        walk_part(Owner::Page(found.id), &found.shapes, true, &mut |sheet, data, _| f(sheet, data));
    }
}

/// The shapes of one part, a group before its members, down to `MAX_SHAPE_DEPTH`.
fn walk_part(owner: Owner, top: &ShapeList, addressable: bool, f: &mut dyn FnMut(&SheetRef, &Sheet, bool)) {
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 0)];
    let mut path: Vec<u32> = Vec::new();
    while let Some((list, next)) = stack.last_mut() {
        let Some(shape) = list.get(*next) else {
            stack.pop();
            path.pop();
            continue;
        };
        let Ok(place) = u32::try_from(*next) else {
            stack.pop();
            path.pop();
            continue;
        };
        *next += 1;
        path.push(place);
        f(&SheetRef::Shape(ShapeRef { owner, path: path.clone() }), &shape.sheet, addressable);
        if path.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0));
        } else {
            path.pop();
        }
    }
}

/// Calls `f` for each cell of a sheet that holds a formula of its own or `Inh`, with its address and whether it is `Inh`.
pub(crate) fn for_each_formula_cell(sheet: &Sheet, mut f: impl FnMut(CellAddr, bool)) {
    let wanted = |slot: &FormulaSlot| match slot {
        FormulaSlot::Local(_) => Some(false),
        FormulaSlot::Inherit => Some(true),
        FormulaSlot::Absent | FormulaSlot::NoFormula => None,
    };
    for (name, cell) in &sheet.cells {
        if let Some(inherited) = wanted(&cell.formula) {
            f(CellAddr { section: None, row: None, cell: name.clone() }, inherited);
        }
    }
    for section in &sheet.sections {
        if section.del {
            continue;
        }
        let key = SectionKey { name: section.name.clone(), ix: section.ix };
        for (name, cell) in &section.cells {
            if let Some(inherited) = wanted(&cell.formula) {
                f(CellAddr { section: Some(key.clone()), row: None, cell: name.clone() }, inherited);
            }
        }
        for row in &section.rows {
            if row.del {
                continue;
            }
            let Some(row_key) = row.key(section.name.as_str()) else { continue };
            for (name, cell) in &row.cells {
                if let Some(inherited) = wanted(&cell.formula) {
                    f(CellAddr { section: Some(key.clone()), row: Some(row_key.clone()), cell: name.clone() }, inherited);
                }
            }
        }
    }
}

/// Where a sheet is, for a difference. A shape's own `ID` is looked up, once, for the difference that is listed.
fn place_of(ev: &Evaluator<'_, '_>, sheet: &SheetRef) -> SheetPlace {
    match sheet {
        SheetRef::Document => SheetPlace::Document,
        SheetRef::Style(id) => SheetPlace::Style(*id),
        SheetRef::Page(id) => SheetPlace::Page(*id),
        SheetRef::Master(id) => SheetPlace::Master(*id),
        SheetRef::Shape(at) => {
            SheetPlace::Shape { owner: at.owner, id: ev.res.shape(at).map(|s| s.id), depth: u32::try_from(at.path.len()).unwrap_or(u32::MAX) }
        }
    }
}

// ---- the clock walk ---------------------------------------------------------------------------------------------------------

/// A step of the walk of what a cell reads: a cell, or the text of a sheet (which reads the sheet's `Field` values).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Node {
    Cell(CellKey),
    Text(SheetRef),
}

/// State kept across the cells of one report.
#[derive(Default)]
struct Walker {
    /// Whether each node is a clock reader by what it names, found once. Each entry is held memory.
    taint: HashMap<Node, bool>,
    /// The choice of cells to salt, when the run salts (`salt.rs`).
    salt: Option<SaltRun>,
}

/// Works out one cell and adds it to the report.
fn score_cell(
    ev: &mut Evaluator<'_, '_>,
    calc: &mut dyn Calculator,
    walker: &mut Walker,
    report: &mut RecalcReport,
    sheet: &SheetRef,
    addr: CellAddr,
    inherited: bool,
) {
    report.cells += 1;
    if ev.ctx.stats.budget_exhausted {
        report.unreached += 1;
        report.budget_exhausted = true;
        return;
    }
    let at = CellAddress { sheet: sheet.clone(), addr: addr.clone() };
    let outcome = calc.calculate(ev, &at);
    if ev.ctx.stats.budget_exhausted {
        report.unreached += 1;
        report.budget_exhausted = true;
        return;
    }
    match outcome.kind {
        CellKind::Action => {
            report.skipped.actions += 1;
            return;
        }
        CellKind::ParseFailed => {
            report.skipped.parse_failed += 1;
            return;
        }
        // The chain holds no formula to work out (an `Inh` cell whose master holds a constant, or nothing), or only the default formula
        // a page prints for a cell nothing holds: there is nothing to compare the saved value with.
        CellKind::Constant | CellKind::NoFormula | CellKind::Missing => {
            report.skipped.inherited_no_formula += 1;
            return;
        }
        CellKind::Formula if outcome.default_formula => {
            report.skipped.inherited_no_formula += 1;
            return;
        }
        CellKind::Formula => {}
    }
    // The clock comes before the unsupported functions: a cell that reads the clock is skipped as that, whether or not a clock was given
    // and so whether or not the function that reads it gave a value (`TEXTHEIGHT` of a text with `NOW` in it is skipped).
    let clock_reader = outcome.tainted || reads_clock_by_links(ev, walker, &at);
    if ev.ctx.stats.budget_exhausted {
        report.unreached += 1;
        report.budget_exhausted = true;
        return;
    }
    if clock_reader {
        if let Some(fit) = check_document_date(ev, &at, &outcome) {
            if fit {
                report.document_date_checks.fit += 1;
            } else {
                report.document_date_checks.not_fit += 1;
            }
        }
        report.skipped.clock_readers += 1;
        return;
    }
    if outcome.depth_limit_hits > 0 {
        report.skipped.depth_limit += 1;
        return;
    }
    let unsupported = match (&outcome.unsupported, &outcome.raw) {
        (Some(name), _) => Some(name.to_string()),
        (None, Value::Unsupported(name)) => Some(name.to_string()),
        _ => None,
    };
    if let Some(name) = unsupported {
        // An operand saved as a marker that is no value (`Themed`, a date past 9999) is not a function this evaluator lacks: the cell is left
        // out under a reason of its own, and `unsupported` keeps only the names of functions.
        if name == THEMED_MARKER {
            report.skipped.themed_operand += 1;
            return;
        }
        if name == FAR_YEAR {
            report.skipped.far_year_operand += 1;
            return;
        }
        let key = if report.unsupported.contains_key(&name) || report.unsupported.len() < MAX_UNSUPPORTED_NAMES {
            name
        } else {
            OTHER_UNSUPPORTED.to_owned()
        };
        *report.unsupported.entry(key).or_default() += 1;
        return;
    }
    // What the file saved that stands to be compared: a typed `V`, or an `E`. A `Themed` `V` or a date past the year 9999 is no value to
    // compare, and with no error saved either the cell is left out; with one, the error is compared (the value is not).
    let error_saved = outcome.saved_error.is_some();
    match &outcome.saved {
        Some(Value::Unsupported(marker)) if &**marker == FAR_YEAR && !error_saved => {
            report.skipped.far_year += 1;
            return;
        }
        Some(Value::Unsupported(marker)) if &**marker != FAR_YEAR && !error_saved => {
            report.skipped.themed += 1;
            return;
        }
        None if !error_saved => {
            report.skipped.nothing_saved += 1;
            return;
        }
        _ => {}
    }
    // A formula that can only fail, and does (a project rule): the file saves a value and no `E`, and the cell's own formula reached a
    // `REF()` or a cell its section can never hold, and that `#REF!` is its result. A `#REF!` from a row, shape, page or master the file lacks is
    // not this and stays scored, as a lookup that is wrong would look the same.
    if !error_saved && outcome.must_fail && matches!(outcome.raw, Value::Error(CellError::Ref)) {
        report.skipped.formula_must_fail += 1;
        return;
    }
    let facts = facts_of_cell(ev, &at);
    let functions: &[String] = facts.as_ref().map_or(&[], |f| f.functions.as_slice());
    // A step for each function the formula names, before they are counted: a formula of 5,000 names is 5,000 steps a cell.
    if !ev.steps(functions.len()) {
        report.unreached += 1;
        report.budget_exhausted = true;
        return;
    }
    // A font cell that gives a number other than 0 has the number of the machine that saved the file, which nothing in the file records
    // (a project rule, as the clock readers are left out).
    if is_machine_font_number(&addr, &outcome) {
        report.skipped.machine_font_number += 1;
        return;
    }
    // A text measured in a stand-in face has the stand-in's widths, which the file's do not follow.
    if outcome.stand_in {
        report.skipped.text_stand_in += 1;
        return;
    }
    let compare_with = comparison_for(functions);
    // A colour under a scheme the file no longer selects (narrowed: a project rule). The cell has been worked out as its sheet reads it,
    // with no theme. It is left out only when that reading does not give the saved value and a second one does, with the file's first theme part selected:
    // the saved value is then the stale kind. Everything else under the condition is scored, as a pass or a difference, by the first reading.
    let first_judged = match stale_colour(ev, calc, &at, functions, &outcome, compare_with) {
        Stale::LeftOut => {
            report.skipped.stale_theme_colour += 1;
            tally_stale(report, functions);
            return;
        }
        Stale::Cut => {
            report.unreached += 1;
            report.budget_exhausted = true;
            return;
        }
        Stale::Scored(first) => first,
    };
    // A shadow property the theme answered on the branch where its effect entry holds no outer shadow: files save another colour there and the
    // render rule stays `#000000` (a project rule). The entry decides, and not the value saved.
    if outcome.no_shadow {
        report.skipped.theme_no_shadow += 1;
        return;
    }
    let judged = first_judged.unwrap_or_else(|| judge(ev, &at, &outcome, compare_with));
    let verdict = judged.difference;
    if verdict.is_none() && !agrees_strictly(ev.doc, &outcome) {
        report.agreed_by_allowance += 1;
    }
    if verdict.is_none() && judged.by_curve {
        report.agreed_by_curve += 1;
    }
    // Listing a difference prints both values, a pass over each text: charged by its length (a title shared by many cells is no free copy).
    if verdict.is_some() && report.differed.len() < MAX_DIFFERENCES {
        let printed = printed_len(&outcome.raw) + outcome.saved.as_ref().map_or(0, printed_len);
        if !ev.work(printed) {
            report.unreached += 1;
            report.budget_exhausted = true;
            return;
        }
    }
    for f in functions {
        // The name is cloned only the first time it is met; past the cap the score goes under `(other)`.
        if !report.by_function.contains_key(f) {
            let key = if report.by_function.len() < MAX_UNSUPPORTED_NAMES { f.clone() } else { OTHER_UNSUPPORTED.to_owned() };
            report.by_function.entry(key).or_default();
        }
        let key = if report.by_function.contains_key(f) { f.as_str() } else { OTHER_UNSUPPORTED };
        if let Some(s) = report.by_function.get_mut(key) {
            if verdict.is_none() {
                s.agreed += 1;
            } else {
                s.differed += 1;
            }
        }
    }
    tally_cell(report, &addr, verdict.is_none());
    if verdict.is_none()
        && let Some(run) = walker.salt.as_mut()
    {
        run.offer(sheet, &addr, &outcome, compare_with.tolerance, compare_with.colour_step);
    }
    match verdict {
        None => report.agreed += 1,
        Some(_) if report.differed.len() >= MAX_DIFFERENCES => report.differed_unlisted += 1,
        Some(kind) => {
            let place = place_of(ev, sheet);
            // The value the evaluation gave. When the formula failed or the function is missing, the saved one stands.
            let shown = match &outcome.raw {
                Value::Error(_) | Value::Unsupported(_) => outcome.saved.as_ref(),
                v => Some(v),
            };
            report.differed.push(Difference {
                sheet: place.label(),
                cell: cell_label(&addr),
                place,
                address: addr,
                inherited,
                functions: functions.iter().take(MAX_DIFFERENCE_FUNCTIONS).cloned().collect(),
                kind,
                saved: outcome.saved.as_ref().map(print_value),
                saved_class: outcome.saved.as_ref().and_then(class_of),
                saved_error: outcome.saved_error.map(|e| e.text().to_owned()),
                actual: shown.map(print_value),
                actual_class: shown.and_then(class_of),
                actual_error: outcome.error.map(|e| e.text().to_owned()),
            });
        }
    }
}

/// Works out again, after the walk, the cells the salt chose, each with a wrong saved value put in, and compares them as the walk did. The
/// counters are put back after, so the salt is in no figure of the score and cannot end the run as `budget_exhausted`; a cell is not
/// worked out when the budget the walk left is spent (`RealSalt::not_checked`).
fn work_out_the_salt(ev: &mut Evaluator<'_, '_>, calc: &mut dyn Calculator, run: SaltRun) -> RealSalt {
    let mut out = RealSalt { too_large: run.too_large, ..RealSalt::default() };
    let before = ev.ctx.counters();
    for item in run.plan {
        if ev.ctx.stats.budget_exhausted {
            out.not_checked += 1;
            continue;
        }
        let class = item.salted.class;
        let at = CellAddress { sheet: item.key.0.clone(), addr: item.key.1.clone() };
        ev.ctx.salt = Some((item.key, item.salted));
        let again = calc.calculate(ev, &at);
        ev.ctx.salt = None;
        // A cell that spent what the walk left is not a verdict either way.
        if ev.ctx.stats.budget_exhausted {
            out.not_checked += 1;
            continue;
        }
        let how = Comparison { tolerance: item.tolerance, colour_step: item.colour_step, relative: true };
        let judged = judge(ev, &at, &again, how);
        out.salted += 1;
        let caught = judged.difference.is_some();
        // The curve excuses a salted cell only where a wrong value could agree by design: a circle's angle, or the other end of an axis. Any wider and
        // the comparer would be agreeing a wrong value, which is a miss.
        match (caught, judged.by_atan2 || (judged.by_curve && judged.curve_by_design)) {
            (true, _) => out.caught += 1,
            (false, true) => out.by_allowance += 1,
            (false, false) => {
                out.missed += 1;
                if out.misses.len() < MAX_MISSES_KEPT {
                    let functions: Vec<String> =
                        facts_of_cell(ev, &at).map_or_else(Vec::new, |f| f.functions.iter().take(MAX_DIFFERENCE_FUNCTIONS).cloned().collect());
                    out.misses.push(SaltMiss { place: place_of(ev, &at.sheet), address: at.addr.clone(), functions, class });
                }
            }
        }
        out.tally(class, caught);
    }
    ev.ctx.restore_counters(before);
    out
}

/// Adds a scored cell to the tally of its kind: its section's name (empty for none) and its own, with no row.
fn tally_cell(report: &mut RecalcReport, addr: &CellAddr, agreed: bool) {
    let key = (addr.section.as_ref().map_or_else(String::new, |s| s.name.as_str().to_owned()), addr.cell.as_str().to_owned());
    let key = if report.by_cell.contains_key(&key) || report.by_cell.len() < MAX_UNSUPPORTED_NAMES {
        key
    } else {
        (OTHER_UNSUPPORTED.to_owned(), OTHER_UNSUPPORTED.to_owned())
    };
    let s = report.by_cell.entry(key).or_default();
    if agreed {
        s.agreed += 1;
    } else {
        s.differed += 1;
    }
}

/// About how many bytes printing a value passes over: its text, or eight bytes for each number of a constructor.
fn printed_len(v: &Value) -> usize {
    match v {
        Value::Text(t) => t.len(),
        Value::Constructor(c) => c.numbers.len().saturating_mul(8),
        _ => 0,
    }
}

/// A value as a report prints it: `V` and, when there is one, `U` after a space, cut at [`MAX_PRINTED_VALUE`] characters.
fn print_value(v: &Value) -> String {
    let full = match encode(v) {
        Some(e) => match e.u {
            Some(u) => format!("{} [{u}]", e.v),
            None => e.v,
        },
        None => v.kind_name().to_owned(),
    };
    cap_printed(full)
}

/// A printed value cut at [`MAX_PRINTED_VALUE`] characters, with its length after `...` when it was cut.
pub(crate) fn cap_printed(s: String) -> String {
    match s.char_indices().nth(MAX_PRINTED_VALUE) {
        None => s,
        Some((at, _)) => format!("{}...({} characters)", &s[..at], s.chars().count()),
    }
}

/// A cell as a report names it.
fn cell_label(addr: &CellAddr) -> String {
    let mut s = String::new();
    if let Some(section) = &addr.section {
        s.push_str(section.name.as_str());
        if let Some(ix) = section.ix {
            s.push_str(&format!("[{ix}]"));
        }
        s.push('.');
    }
    match &addr.row {
        Some(RowKey::Ix(n)) => s.push_str(&format!("IX={n}.")),
        Some(RowKey::Name(n)) => s.push_str(&format!("{n}.")),
        None => {}
    }
    s.push_str(addr.cell.as_str());
    s
}

/// What a cell's formula holds (the formula that runs: the local one, or the one it inherits), found once per formula: among the facts, the
/// functions it calls, in capitals, each once.
fn facts_of_cell(ev: &mut Evaluator<'_, '_>, at: &CellAddress) -> Option<std::sync::Arc<crate::facts::Facts>> {
    let res = ev.res.resolve(&at.sheet, &at.addr)?;
    let FormulaSlot::Local(formula) = res.formula.cell.map(|c| &c.formula)? else { return None };
    ev.facts_of(formula)
}

/// How the cell of a formula that calls `functions` is compared: the standard tolerance, raised for the text measures (1E-4),
/// the path functions (2E-5) and the geometry batch (1E-6), the largest of those that apply; and a colour step for `MSOTINT`.
///
/// The checks run from the largest tolerance to the smallest, so that `raise` taking the larger of the two is what keeps a smaller tolerance
/// from lowering a larger one: with an assignment in its place a formula that calls a text measure and a geometry function would be compared
/// to 1E-6 (`a_comparison_takes_the_largest_tolerance_of_the_functions_in_any_order`).
fn comparison_for(functions: &[String]) -> Comparison {
    let mut how = Comparison::standard();
    let raise = |how: &mut Comparison, to: f64| how.tolerance = how.tolerance.max(to);
    if functions.iter().any(|f| f == "TEXTWIDTH" || f == "TEXTHEIGHT") {
        raise(&mut how, TEXT_MEASURE_COMPARE_TOLERANCE);
    }
    if functions.iter().any(|f| is_path_function(f)) {
        raise(&mut how, PATH_COMPARE_TOLERANCE);
    }
    // The two reserved functions of an elliptical arc agree at 1E-6 and at 2E-5 alike: the tighter.
    if functions.iter().any(|f| is_geometry_function(f) || f == "_ELLIPSE_THETA" || f == "_ELLIPSE_ECC") {
        raise(&mut how, GEOMETRY_COMPARE_TOLERANCE);
    }
    if functions.iter().any(|f| f == "MSOTINT") {
        how.colour_step = 1;
    }
    how
}

/// The nine functions that walk a path or measure a box (`fn_path.rs`), which the oracle compares to 2E-5.
fn is_path_function(name: &str) -> bool {
    matches!(
        name,
        "ANGLEALONGPATH"
            | "BOUNDINGBOXDIST"
            | "BOUNDINGBOXRECT"
            | "DISTTOPATH"
            | "NEARESTPOINTONPATH"
            | "PATHLENGTH"
            | "PATHSEGMENT"
            | "POINTALONGPATH"
            | "SEGMENTCOUNT"
    )
}

/// The geometry batch's functions, which the oracle compares to 1E-6.
fn is_geometry_function(name: &str) -> bool {
    matches!(
        name,
        "ANGLEALONGPATH"
            | "ANGLETOLOC"
            | "ANGLETOPAR"
            | "BOUNDINGBOXDIST"
            | "BOUNDINGBOXRECT"
            | "DISTTOPATH"
            | "GRAVITY"
            | "INTERSECTX"
            | "INTERSECTY"
            | "LOC"
            | "LOCTOLOC"
            | "LOCTOPAR"
            | "NEARESTPOINTONPATH"
            | "PAR"
            | "PATHLENGTH"
            | "PATHSEGMENT"
            | "PNT"
            | "PNTX"
            | "PNTY"
            | "POINTALONGPATH"
            | "RECTSECT"
            | "SEGMENTCOUNT"
    )
}

/// Whether the cell reads, by name and in any branch, a cell that calls a clock function or reads one in turn, or the text of a sheet
/// whose fields do. The cell's own direct calls are not counted: those are decided by what its
/// evaluation reached, so `IF(1=1,5,NOW())` is scored.
fn reads_clock_by_links(ev: &mut Evaluator<'_, '_>, walker: &mut Walker, at: &CellAddress) -> bool {
    let root = Node::Cell((at.sheet.clone(), at.addr.clone()));
    let mut links: Vec<Node> = Vec::new();
    if expand(ev, &root, true, &mut links) {
        return true;
    }
    for link in links {
        if taint_of(ev, walker, link) {
            return true;
        }
        if ev.ctx.stats.budget_exhausted {
            return false;
        }
    }
    false
}

/// What a node leads to: the cells a cell's formula names, and the sheets whose text it reads; for a sheet's text, its `Field` values.
/// True when the cell itself names a clock function (a root's own calls are not counted). Each step is charged.
fn expand(ev: &mut Evaluator<'_, '_>, node: &Node, is_root: bool, out: &mut Vec<Node>) -> bool {
    match node {
        Node::Cell(key) => {
            if is_action_cell(&key.1) {
                return false;
            }
            let Some(res) = ev.res.resolve(&key.0, &key.1) else { return false };
            let Some(FormulaSlot::Local(formula)) = res.formula.cell.map(|c| &c.formula) else { return false };
            let Some(facts) = ev.facts_of(formula) else { return false };
            if !is_root && facts.calls_clock {
                return true;
            }
            let env = Env::new(res.on.clone(), res.rebind.clone());
            for r in &facts.refs {
                if !ev.steps(1) {
                    return false;
                }
                if let Ok((sheet, addr)) = ev.locate(r, &env) {
                    out.push(Node::Cell((sheet, addr)));
                }
            }
            for read in &facts.text_reads {
                let target = match read {
                    TextRead::Own => Some(res.on.clone()),
                    // The text of the sheet the reference names, and not the formula's own (`SHAPETEXT(Sheet.1!TheText)`).
                    TextRead::Of(r) => ev.locate(r, &env).ok().map(|(sheet, _)| sheet),
                };
                if let Some(sheet) = target {
                    out.push(Node::Text(sheet));
                }
            }
            false
        }
        Node::Text(sheet) => {
            let fields = SectionKey::new("Field", None);
            if let Some(rows) = ev.res.effective_rows(sheet, &fields) {
                for row in rows {
                    if !ev.steps(1) {
                        return false;
                    }
                    out.push(Node::Cell((sheet.clone(), CellAddr::in_row("Field", None, row.key.clone(), "Value"))));
                }
            }
            false
        }
    }
}

/// Whether a node is a clock reader by what it names, to any depth, with a stack and not by recursion. A walk that finds one remembers every
/// node on the path to it; one that finds none remembers every node it visited. A walk charges a step a node and holds memory a node, and
/// ends at the budget's.
fn taint_of(ev: &mut Evaluator<'_, '_>, walker: &mut Walker, start: Node) -> bool {
    if let Some(known) = walker.taint.get(&start) {
        return *known;
    }
    let mut visited: HashSet<Node> = HashSet::new();
    let mut parent: HashMap<Node, Node> = HashMap::new();
    let mut stack = vec![start.clone()];
    let mut links: Vec<Node> = Vec::new();
    let mut held: u64 = KEY_BYTES;
    let mut found_at: Option<Node> = None;
    let _ = ev.ctx.hold_bytes(KEY_BYTES);
    while let Some(node) = stack.pop() {
        if !ev.steps(1) {
            break;
        }
        match walker.taint.get(&node) {
            Some(true) => {
                found_at = Some(node);
                break;
            }
            Some(false) => continue,
            None => {}
        }
        if visited.contains(&node) {
            continue;
        }
        if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
            break;
        }
        held += 2 * KEY_BYTES;
        visited.insert(node.clone());
        links.clear();
        if expand(ev, &node, false, &mut links) {
            found_at = Some(node);
            break;
        }
        for link in links.drain(..) {
            if visited.contains(&link) {
                continue;
            }
            if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                break;
            }
            held += 2 * KEY_BYTES;
            parent.entry(link.clone()).or_insert_with(|| node.clone());
            stack.push(link);
        }
    }
    let exhausted = ev.ctx.stats.budget_exhausted;
    let found = found_at.is_some();
    if let Some(mut node) = found_at {
        // Every node on the path from the one that named a clock function back to the start reads one.
        for _ in 0..=visited.len() {
            if !walker.taint.contains_key(&node) {
                if !ev.ctx.hold_bytes(TAINT_ENTRY_BYTES) {
                    break;
                }
                walker.taint.insert(node.clone(), true);
            }
            if node == start {
                break;
            }
            match parent.get(&node) {
                Some(p) => node = p.clone(),
                None => break,
            }
        }
    } else if !exhausted {
        // Everything the walk visited reads no clock: all of it was reached from the start, and the walk found none.
        for node in visited {
            if !ev.ctx.hold_bytes(TAINT_ENTRY_BYTES) {
                break;
            }
            walker.taint.insert(node, false);
        }
    }
    ev.ctx.release_bytes(held);
    found
}

/// Whether a cell is a font cell (`Font`, `AsianFont`, `ComplexScriptFont`: the cells of the catalogue's `vFont` kind) and its evaluation gave a
/// number other than 0. A face is named by a number the application that saved the file gave it, and it changes from system to system
/// So the number cannot be compared with the face name the file saved.
fn is_machine_font_number(addr: &CellAddr, outcome: &CellEval) -> bool {
    // The number first: most cells give none, and the catalogue is searched only for those that do.
    matches!(&outcome.raw, Value::Number(q) if q.stored != 0.0)
        && cell_info(addr.section.as_ref().map(|s| s.name.as_str()), addr.cell.as_str()).kind == ValueKind::Font
}

/// Whether a cell is under the condition for a stale colour: it calls `THEMEVAL`, its sheet's `ColorSchemeIndex` reads 65534 (the page's), no cell of
/// the page gives one so the scheme reads as none, and the file holds a theme part. **The condition alone leaves
/// nothing out** (narrowed the rule, which hid 22,976 cells that agree to protect 282): [`theme_reading`] says whether the saved value is the
/// theme's reading and not the sheet's own.
fn is_stale_theme_colour(ev: &mut Evaluator<'_, '_>, at: &CellAddress, functions: &[String], outcome: &CellEval) -> bool {
    if !functions.iter().any(|f| f == "THEMEVAL") || ev.doc.themes.is_empty() {
        return false;
    }
    let colour = matches!(outcome.raw, Value::Color(_)) || matches!(outcome.saved, Some(Value::Color(_)));
    if !colour {
        return false;
    }
    let raw = ev.read_cell(&at.sheet, &CellAddr::direct("ColorSchemeIndex")).view.quantity().map(|q| q.stored);
    raw == Some(65534.0) && ev.scheme_cell(&at.sheet, "ColorSchemeIndex") == 0.0
}

/// Adds a cell left out as stale to the tally by function: a name once for the cell, cloned the first time it is met; past the cap the tally goes under
/// `(other)`.
fn tally_stale(report: &mut RecalcReport, functions: &[String]) {
    for f in functions {
        let tally = &mut report.stale_theme_colour_by_function;
        let key = if tally.contains_key(f) || tally.len() < MAX_UNSUPPORTED_NAMES { f.as_str() } else { OTHER_UNSUPPORTED };
        match tally.get_mut(key) {
            Some(n) => *n += 1,
            None => {
                tally.insert(key.to_owned(), 1);
            }
        }
    }
}

/// What the narrowed rule says of a cell.
enum Stale {
    /// Left out: the saved value is the file's first theme part's reading and not the sheet's own.
    LeftOut,
    /// The budget ran out in the second reading.
    Cut,
    /// Scored. The first reading's judgement comes with it when the cell was under the rule's condition, so that it is not made again.
    Scored(Option<Judged>),
}

/// Applies the narrowed rule (a project rule) to a cell worked out as its sheet reads it: under the rule's condition, and a first reading that
/// differs from the saved value, the second reading decides whether the cell is left out; every other cell is scored.
fn stale_colour(
    ev: &mut Evaluator<'_, '_>,
    calc: &mut dyn Calculator,
    at: &CellAddress,
    functions: &[String],
    outcome: &CellEval,
    how: Comparison,
) -> Stale {
    if !is_stale_theme_colour(ev, at, functions, outcome) {
        return Stale::Scored(None);
    }
    let first = judge(ev, at, outcome, how);
    if first.difference.is_none() {
        return Stale::Scored(Some(first));
    }
    match theme_reading(ev, calc, at, how) {
        ThemeReading::Explains => Stale::LeftOut,
        ThemeReading::Cut => Stale::Cut,
        ThemeReading::DoesNot => Stale::Scored(Some(first)),
    }
}

/// What the second reading of a colour that may be stale found.
enum ThemeReading {
    /// The file's first theme part gives the saved value: the saved value is the stale kind.
    Explains,
    /// It gives something else, or nothing: the first reading's difference stands.
    DoesNot,
    /// The budget ran out in the second reading, so it says nothing either way.
    Cut,
}

/// The second reading of the narrowing (a project rule): the cell worked out by `calc` again with the file's first theme
/// part selected ([`Evaluator::under_first_theme_part`]) and compared with the saved value by the same comparer. A part that cannot answer (the reader left
/// the list out, a function this evaluator lacks) explains nothing. **Cost:** in the cached mode about the evaluation of the cell again; in the live mode
/// the cells it reads are worked out under the second reading too, once for the whole walk (the memo of the second reading outlives the cell:
/// `EvalCtx::theme_memo`), so the walk costs about twice what the first reading alone does, whatever the length of a chain of cells. Its steps are the run's.
fn theme_reading(ev: &mut Evaluator<'_, '_>, calc: &mut dyn Calculator, at: &CellAddress, how: Comparison) -> ThemeReading {
    let second = ev.under_first_theme_part(|ev| calc.calculate(ev, at));
    if ev.ctx.stats.budget_exhausted {
        return ThemeReading::Cut;
    }
    if second.unsupported.is_some() || matches!(second.raw, Value::Unsupported(_)) {
        return ThemeReading::DoesNot;
    }
    if judge(ev, at, &second, how).difference.is_none() { ThemeReading::Explains } else { ThemeReading::DoesNot }
}

/// For a cell whose whole formula is `DOCCREATION()`, `DOCLASTSAVE()`, `DOCLASTEDIT()` or `DOCLASTPRINT()`: whether its saved `V` fits the
/// quarter-hour check against the core property. `None` for any other cell.
fn check_document_date(ev: &mut Evaluator<'_, '_>, at: &CellAddress, outcome: &CellEval) -> Option<bool> {
    let res = ev.res.resolve(&at.sheet, &at.addr)?;
    let FormulaSlot::Local(formula) = res.formula.cell.map(|c| &c.formula)? else { return None };
    let Ok(Expr::Call(call)) = &formula.ast else { return None };
    if !call.args.is_empty() || !call.qualifiers.is_empty() {
        return None;
    }
    let name = call.name.to_ascii_uppercase();
    let props = &ev.doc.props;
    let property = match name.as_str() {
        "DOCCREATION" => props.created.as_ref(),
        "DOCLASTSAVE" | "DOCLASTEDIT" => props.modified.as_ref(),
        "DOCLASTPRINT" => props.last_printed.as_ref(),
        _ => return None,
    }?;
    // A date is a few characters; a property of any other size is not one (and is not trimmed to find out).
    if property.len() > 128 {
        return None;
    }
    let property = iso_to_serial(property)?;
    let Some(Value::Number(saved)) = &outcome.saved else { return Some(false) };
    let diff = (property - saved.stored) * 86_400.0;
    // The writer's zone is a whole number of quarter hours within 14 hours; what is left is the clock's drift.
    let shift = (diff / 900.0).round() * 900.0;
    let left = diff - shift;
    let slack = 0.01;
    let fits =
        shift.abs() <= 14.0 * 3600.0 && if name == "DOCLASTEDIT" { (-slack..=180.0 + slack).contains(&left) } else { left.abs() <= 2.0 + slack };
    Some(fits)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::rules::ORACLE_TOLERANCE;

    fn tolerance(names: &[&str]) -> f64 {
        comparison_for(&names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>()).tolerance
    }

    /// A tolerance is only ever raised: a formula that calls functions of two batches is compared by the looser of the two, whatever order the
    /// names come in. The checks in `comparison_for` run from the largest tolerance down (1E-4, 2E-5, 1E-6), so an assignment where it takes the
    /// larger would let a smaller tolerance lower a larger one: a text measure beside a geometry function would come out at 1E-6.
    #[test]
    fn a_comparison_takes_the_largest_tolerance_of_the_functions_in_any_order() {
        let (geometry, path, text) = ("PNTX", "PATHLENGTH", "TEXTWIDTH");
        assert_eq!(tolerance(&[]), ORACLE_TOLERANCE);
        assert_eq!(tolerance(&["SUM", "MAX"]), ORACLE_TOLERANCE);
        assert_eq!(tolerance(&[geometry]), GEOMETRY_COMPARE_TOLERANCE);
        assert_eq!(tolerance(&[path]), PATH_COMPARE_TOLERANCE);
        assert_eq!(tolerance(&[text]), TEXT_MEASURE_COMPARE_TOLERANCE);
        assert_eq!(tolerance(&["TEXTHEIGHT"]), TEXT_MEASURE_COMPARE_TOLERANCE);
        for order in [
            [geometry, path, text],
            [geometry, text, path],
            [path, geometry, text],
            [path, text, geometry],
            [text, geometry, path],
            [text, path, geometry],
        ] {
            assert_eq!(tolerance(&order), TEXT_MEASURE_COMPARE_TOLERANCE, "{order:?}");
            assert_eq!(tolerance(&[order[0], order[1]]), tolerance(&[order[1], order[0]]), "{order:?}: two names, either way");
        }
        // The largest wins and the ones below it do not matter: the looser of a path function and a geometry one is the path's.
        assert_eq!(tolerance(&[path, geometry]), PATH_COMPARE_TOLERANCE);
        assert_eq!(tolerance(&[geometry, path]), PATH_COMPARE_TOLERANCE);
        assert_eq!(tolerance(&[text, "SUM"]), TEXT_MEASURE_COMPARE_TOLERANCE);
        // The colour step changes no tolerance and the tolerance changes no step.
        let msotint = comparison_for(&["MSOTINT".to_owned(), path.to_owned()]);
        assert_eq!((msotint.tolerance, msotint.colour_step), (PATH_COMPARE_TOLERANCE, 1));
        assert_eq!(comparison_for(&[path.to_owned()]).colour_step, 0);
    }

    /// The real-file salt excuses a salted `C` or `D` that agrees through the curve only for two circles, or an angle a half turn off with
    /// the same ratio, and the test of that is fixed, not the comparison's tolerance. So a curve rule made too loose (here, the planned tolerance raised to
    /// 0.45, which is far from raw agreement for both salted values, and agrees the wrong angle and the wrong ratio as ellipses) shows as a salt miss in both
    /// cells, where the honest tolerance catches both.
    #[test]
    fn a_curve_rule_made_too_loose_shows_as_a_salt_miss() {
        use diacraft_doc::RowKey;
        use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

        use crate::fn_ellipse::{ecc, theta};
        use crate::salt::SaltPolicy;

        let resize = [0.5, 2.0, 4.0, 2.0, 8.0, 2.0];
        let c = theta(&resize).unwrap();
        let d = ecc(&resize, c).unwrap();
        let key = RowKey::Ix(2);
        let shape = ShapeBuilder::new(1)
            .cell("Width", "1.5")
            .cell("Height", "2")
            .row_type("Geometry", Some(0), RowKey::Ix(1), "MoveTo")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "X", "0")
            .row_cell("Geometry", Some(0), RowKey::Ix(1), "Y", "0")
            .row_type("Geometry", Some(0), key.clone(), "EllipticalArcTo")
            .row_formula("Geometry", Some(0), key.clone(), "C", "_ELLIPSE_THETA(0.5,2,4,2,8,2)", &c.to_string())
            .row_formula("Geometry", Some(0), key, "D", "_ELLIPSE_ECC(0.5,2,4,2,8,2,Geometry1.C2)", &d.to_string());
        let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
        let sheet = doc.find_shape(diacraft_doc::Owner::Page(1), 1).map(SheetRef::Shape).unwrap();
        let options = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
        let salted_with = |tolerance: Option<f64>| {
            let mut ctx = EvalCtx::new(options.clone(), Budget::default());
            let mut ev = Evaluator::new(&doc, &mut ctx);
            let mut run = SaltRun::new(SaltPolicy { seed: 7, one_in: 1 });
            for name in ["C", "D"] {
                let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), name);
                let outcome = Native.calculate(&mut ev, &CellAddress { sheet: sheet.clone(), addr: addr.clone() });
                run.offer(&sheet, &addr, &outcome, GEOMETRY_COMPARE_TOLERANCE, 0);
            }
            assert_eq!(run.plan.len(), 2, "both cells agreed and are salted");
            for planned in &mut run.plan {
                planned.tolerance = tolerance.unwrap_or(planned.tolerance);
            }
            work_out_the_salt(&mut ev, &mut Native, run)
        };
        let honest = salted_with(None);
        assert_eq!((honest.salted, honest.caught, honest.missed, honest.by_allowance), (2, 2, 0, 0), "{honest:?}");
        let loose = salted_with(Some(0.45));
        assert_eq!((loose.salted, loose.caught, loose.missed, loose.by_allowance), (2, 0, 2, 0), "{loose:?}");
        assert!(!loose.passed(), "a rule made too loose agrees a wrong value, and the salt says so: {loose:?}");
    }

    /// The memo of the second reading outlives a cell (that is what keeps a chain linear), so a caller that changes the document and calls `invalidate` must lose
    /// it too, and the memory it held: a stale colour of the old document would otherwise answer for the new one.
    #[test]
    fn invalidate_lets_go_of_the_memo_of_the_second_reading_and_its_bytes() {
        use diacraft_doc::RowKey;
        use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
        use diacraft_testkit::theme::ThemeXml;

        let shape = ["ThemeIndex", "ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex"]
            .iter()
            .fold(ShapeBuilder::new(5), |s, n| s.cell(n, "65534"))
            .row_formula("User", None, RowKey::name("A"), "Value", r#"THEMEVAL("Light")"#, "#3a6b9e")
            .row_formula("User", None, RowKey::name("B"), "Value", "User.A", "#3a6b9e");
        let mut doc = DocBuilder::new().master(1, MasterBuilder::new().shape(shape)).page(1, PageBuilder::new()).build();
        doc.themes.push(ThemeXml::default().part());
        let owner = diacraft_doc::Owner::Master(1);
        let sheet = doc.find_shape(owner, 5).map(SheetRef::Shape).unwrap();
        let at = CellAddress { sheet, addr: CellAddr::in_row("User", None, RowKey::name("B"), "Value") };
        let options = EvalOptions { operands: OperandMode::Live, ..EvalOptions::default() };
        let mut ctx = EvalCtx::new(options, Budget::default());
        {
            let mut ev = Evaluator::new(&doc, &mut ctx);
            let first = Native.calculate(&mut ev, &at);
            let held_by_first = ev.ctx.bytes_held();
            let second = ev.under_first_theme_part(|ev| Native.calculate(ev, &at));
            assert_ne!(first.raw, second.raw, "the two readings differ: no theme and the first part");
            assert!(ev.ctx.theme_memo.len() >= 2 && ev.ctx.bytes_held() > held_by_first, "the cells of the second reading are kept, and held");
            assert!(ev.ctx.memo.len() >= 2, "and the memo of the first reading is back");
        }
        ctx.invalidate();
        assert!(ctx.memo.is_empty() && ctx.theme_memo.is_empty(), "both memos are let go");
        assert_eq!(ctx.bytes_held(), 0, "and so is the memory they held");
    }

    /// The tolerances ascend as the comparison's tests assume: a compile-time fact, so a constant changed out of order is an error of the build.
    #[test]
    fn the_tolerances_ascend_as_the_comparison_assumes() {
        const { assert!(ORACLE_TOLERANCE < GEOMETRY_COMPARE_TOLERANCE && GEOMETRY_COMPARE_TOLERANCE < PATH_COMPARE_TOLERANCE) };
        const { assert!(PATH_COMPARE_TOLERANCE < TEXT_MEASURE_COMPARE_TOLERANCE) };
    }
}
