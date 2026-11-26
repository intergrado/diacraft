//! Reading a ShapeSheet: the sections, rows and cells of a shape, a page, a master, a style or the document, as the window of the ShapeSheet shows them and as
//! `sheet.read`, `sheet.cell`, `sheet.sections` and `sheet.evaluate` answer. The design is the ShapeSheet window's (section 5 for the window, 6 for
//! the budgets, 7 for the commands) and the spec of the window is.
//!
//! The window calls this module directly on the session's document, and the queries are JSON over the same functions: a JSON answer for each scroll of a section of 31,273 rows would
//! be waste. Nothing here writes: a read is a query, and the model is the one source of truth.
//!
//! * [`Place`] names a sheet: the params of every `sheet.*` command, and the object a tab holds, by owner and `ID`.
//! * [`Outline`] is the sheet's sections in the window's order with the keys of their rows and their lines, built with no cell resolved; `show.rs` decides which
//!   display groups show, and the order is the catalogue's own (`diacraft_doc::DISPLAY_ORDER`).
//! * [`read_lines`] resolves the cells of a window of lines (a scroll resolves the new lines' cells and no others).
//! * [`evaluate`](eval::evaluate) tries a text on a sheet and writes nothing.
//!
//! **Budgets**, each held by a test to a count and never a time: a read gives at most [`MAX_LINES_PER_READ`] lines, a text of a cell at most [`MAX_FORMULA_CHARS`] characters, an
//! answer at most [`MAX_ANSWER_BYTES`] of text, an outline at most [`MAX_OUTLINE_LINES`] lines, and a dry run at most [`EVALUATE_STEPS`] steps of the evaluator.

mod cell;
mod eval;
mod lines;
mod outline;
mod show;
mod target;
mod text;

pub use cell::{CellView, FromView, Origin};
pub use eval::{Dry, Fault, cut_text, evaluate, fault_words};
pub use lines::{Line, LineKind, Lines, RowView, read_lines};
pub use outline::{Counts, Options, Outline, SectionInfo, Slot, ViewError, line_at};
pub use target::{ONE_SHEET, Place, Target};
pub use text::CellValue;

pub(crate) use cell::{Ctx, class_word, respell, view};
pub(crate) use eval::fault_of;
pub(crate) use lines::row_key_json;
pub(crate) use outline::{label_of, name_of};
pub(crate) use text::{default_of, kind_word};

/// The most lines one read gives (design 6): `count` is from 1 to this.
pub const MAX_LINES_PER_READ: usize = 2_000;

/// The lines a read gives when it is not told.
pub const DEFAULT_LINES: usize = 200;

/// The most characters of a formula, a value or a shown text a read gives of one cell; `truncated` says it was cut, and `sheet.cell` gives the whole.
pub const MAX_FORMULA_CHARS: usize = 4_096;

/// The most text an answer holds, in bytes as accounted: a read stops after the line that passes it, with `more`.
pub const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;

/// The most lines an outline lists (design 6): a section past it lists the rows that fit and a line that says how many are not listed.
pub const MAX_OUTLINE_LINES: u64 = 1_000_000;

/// The most steps of the evaluator a dry run takes.
pub const EVALUATE_STEPS: u64 = 200_000;

/// The most sections a read may be asked for by name.
pub const MAX_SECTION_NAMES: usize = 256;

/// `count` of a read: a whole number from 1 to [`MAX_LINES_PER_READ`], or the default when it is left out. `None` for a number outside them.
pub fn read_count(count: Option<u64>) -> Option<usize> {
    match count {
        None => Some(DEFAULT_LINES),
        Some(n) if (1..=MAX_LINES_PER_READ as u64).contains(&n) => usize::try_from(n).ok(),
        Some(_) => None,
    }
}
