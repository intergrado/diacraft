//! DiaCraft recalculation: an evaluator of ShapeSheet formulas over the document model, and an oracle that scores it against the values
//! The reference app saved in a file.
//!
//! A viewer draws saved values; this crate shows how far they can be trusted, and later recalculates after an edit. The spec is
//! the language, where a formula runs: on the inheritor and the
//! `functions-*.md` batches; every choice they leave open is a named constant in [`rules`] or says "project rule" where it is made.
//!
//! * [`Evaluator`] works one cell or one expression out. [`evaluate`] is the one-call form of the same.
//! * [`recalc_all`] works out every formula cell of a document and compares each with its saved `V` and `E`, counting
//!   what agreed, what differed and which functions it does not have. [`RecalcReport`] is what the oracle harness of the command line reads.
//! * [`recalc_all_salted`] is that with the **real-file salt** (`salt.rs`): a seeded sample of the cells that agreed is put right out with a wrong
//!   saved value and worked out again, so a score can say that the comparer compares the cells of the file it scores, and that the calculator is
//!   not handing back what the file saved ([`RealSalt`]).
//! * [`commit`] writes results back through the document model's `write_batch` under the `Evaluation` origin.
//! * [`commit_changed`] does that for what an editing command changed and the cells that read it, transitively, and no other cell: [`changes_between`]
//!   finds what changed, and a cell nothing it changed is read by keeps the value the file saved; an edit it cannot follow cell by cell is
//!   worked out as the whole page, measured from the document the edit started from ([`Since`]), so that an instance is given an inherited cell and a
//!   master's cell is worked out only where the edit changed it. [`commit_changed_with`] is that given a
//!   [`ReaderIndex`], which keeps from one edit to the next what reads what through other sheets (and [`changes_between_with`] is [`changes_between`]
//!   given the same index, which remembers the page it compared), so that an edit costs what it touched and not the formulas and the shapes of the page.
//!
//! **What it does not do.** It never evaluates an action cell (`Action` and its kin) and performs no `SETF` write during
//! recalculation. A function it does not have is [`Value::Unsupported`], and the saved value stands.
//!
//! **Hostile input.** A formula, a cell chain and a document are as hostile as the file. Every node and every cell read costs a step
//! of the [`Budget`]; a cycle, a spent budget and a chain too deep are [`rules::LOOP_ERROR`], counted in [`EvalStats`]; nothing
//! recurses on the length of a chain of cells; and nothing in this crate reads a clock, a file or a thread (the web build compiles it).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod affected;
mod cells;
mod changes;
mod commit;
mod compare;
mod convert;
mod cycle;
mod eval;
mod facts;
mod field_text;
mod fn_cells;
mod fn_color;
mod fn_date;
mod fn_doc;
mod fn_ellipse;
mod fn_geom;
mod fn_logic;
mod fn_marker;
mod fn_math;
mod fn_measure;
mod fn_ops;
mod fn_path;
mod fn_text;
mod fn_trig;
mod index;
mod names;
mod ops;
mod pathwalk;
mod picture;
mod recalc;
mod refs;
mod registry;
mod row_names;
pub mod rules;
mod salt;
mod shapes;
mod store;
mod strnum;
mod theme_colour;
mod theme_eval;
mod theme_names;
mod theme_tables;
mod value;
pub mod work;

#[cfg(kani)]
mod kani_proofs;

pub use cells::{CellEval, CellKind};
pub use changes::{Changes, Since, changes_between, changes_between_with};
pub use commit::{CommitReport, Scope, commit, commit_changed, commit_changed_with, commit_in, write_value};
pub use eval::{Budget, CellAddress, Clock, EvalCtx, EvalOptions, EvalStats, Evaluator, FileInfo, OperandMode};
pub use field_text::{clock_at, field_picture, field_text, utc_iso};
pub use fn_measure::Fonts;
pub use index::{MAX_KEPT_BYTES, ReaderIndex};
pub use recalc::{
    Calculator, Difference, DifferenceKind, FunctionScore, Native, NotFormulas, RecalcReport, SheetPlace, Skipped, ValueClass, class_of, recalc_all,
    recalc_all_salted, recalc_all_using, recalc_all_with,
};
pub use registry::function_names;
pub use salt::{MAX_MISSES_KEPT, RealSalt, SaltClass, SaltMiss, SaltPolicy, SaltTally};
pub use store::{Encoded, decode, encode, iso_to_serial, serial_to_iso};
pub use strnum::{format_number, parse_decimal, read_quantity};
pub use value::{CellError, Constructor, ConstructorKind, Point, Rgb, Value};

use diacraft_doc::Document;

/// Works out the cell `at`: [`Evaluator::evaluate`] on a new evaluator of `doc`. A caller that works out many cells keeps an
/// [`Evaluator`], which builds the document's lookups once.
pub fn evaluate(doc: &Document, at: &CellAddress, ctx: &mut EvalCtx) -> Value {
    Evaluator::new(doc, ctx).evaluate(at)
}
