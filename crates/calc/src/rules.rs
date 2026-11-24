//! The project's own rules: choices about how the reference app behaves that no spec settles, and numbers the specs give, each in one place.
//!
//! Each constant names the spec item it comes from, as `crates/geom/src/rules.rs` does, so a rule can be flipped here when the
//! oracle of Task 6's step 4 (a run over many sample files) or a later spec disagrees. A rule that has no flag, because it is a
//! branch in one function, says where that branch is.

use crate::value::CellError;

/// `unknown`: whether a function that is not linear (`INT`, `INTUP`, `ROUND`, `TRUNC`, `MODULUS`, `LN`, `LOG10`,
/// `SQRT`, `POW`) works on the stored internal value or on the number shown in the argument's unit. **`CEILING` and `FLOOR` are not governed by this:**
/// samples settle `CEILING` (the number shown in the unit it carries, 378 cells), and `FLOOR` follows by our rule, so both
/// always work on the number shown.
///
/// **Project rule (A): the number shown in the argument's unit**, as `CEILING` does, and internal units (inches for a
/// length) for `DL` or no unit. **It is our rule, and no sample tests it yet** (a sample of each of the nine with a cached value would settle it).
/// `INT(1.5 ft)` is 1 ft, 12 stored inches; the stored reading of kept 18 inches and read 1.5 ft. A `MODULUS` divisor with no unit counts in the
/// number's unit, as a `CEILING` multiple does. `SQRT` and `POW` are powers of the unit and read the same on both numbers (the unit's dimension carries the power), so **they
/// work on the stored value whatever this says** (the acre and the hectare, whose scale is not raised to the dimension, give a wrong `SQRT` through the number
/// shown), and `POW`'s percent base is the documented case. To flip back: set this to `true`, and the other seven work on the
/// stored value and keep the argument's unit.
pub const NONLINEAR_ON_STORED: bool = false;

/// `inferred-from-sample`: what `THEMEPROP("Embellishment")` gives where any of the five
/// scheme cells reads 0, so the sheet reads no theme. **2**, where MS-VSDX §2.5.3.158 says 0: all 503 cells in the samples that save a number hold 2
/// (154 files, 45 sources, 111 of them in files with no theme part). `Multiformat` stays 0 there. To flip: set this to 0.
pub const EMBELLISHMENT_WITH_NO_THEME: f64 = 2.0;

/// The narrowing (a project rule): the scheme number that stands for "the file's first theme part" while the oracle works a
/// cell out a second time (`Evaluator::under_first_theme_part`, crate-private). It is not 0 (a sheet that reads no theme), not 65534 (follow the
/// page) and not 65535 (a custom colour scheme), and sends a number no part holds to the first part: a scheme number past 1E9 is
/// never read as a part's index, so it holds no part's.
pub const FIRST_THEME_PART: f64 = 2.0e9;

/// The gap the symbol comparisons allow, absolute and in stored units. `FEQ`, `FNE`, `FGE`, `FGT`,
/// `FLE` and `FLT` use it; the exact family and `SIGN`'s default fuzz do not (`SIGN`'s is [`SIGN_FUZZ`]).
pub const COMPARE_TOLERANCE: f64 = 1e-9;

/// The fuzz `SIGN` uses when it has none.
pub const SIGN_FUZZ: f64 = 1e-9;

/// What a cycle gives (`unknown`), what a chain deeper than the limits gives, and what a spent budget gives.
/// **Project rule:** `#VALUE!` for all three, for a loop through `EVALCELL`. The evaluator
/// also counts each in its statistics (`EvalStats`), so a report can tell a cycle from an error the formula made.
pub const LOOP_ERROR: CellError = CellError::Value;

/// How deep `EVALCELL`, `EVALTEXT` and a string read as a formula may nest before the
/// result is [`LOOP_ERROR`]. A project rule (the spec says 64 for `EVALCELL`).
pub const MAX_NESTED_EVALUATIONS: usize = 64;

/// How deep a string read as a formula may nest inside arithmetic (`unknown`). A project rule.
pub const MAX_STRING_FORMULA_DEPTH: usize = 8;

/// The most native frames the evaluator stacks while it works down one formula, across the cells it reads on the way. The parser
/// already limits one formula to a nesting of `diacraft_formula::MAX_NESTING` and a left-leaning chain is walked in a loop, so this
/// is what a chain of cells read one inside another (a sheet-returning call, `EVALCELL`, a string read as a formula) can add. At the
/// size of a frame this is far below 1 MiB, the stack a browser gives the web build. A project rule.
pub const MAX_NATIVE_DEPTH: usize = 160;

/// How many levels of [`MAX_NATIVE_DEPTH`] a cell read in place counts for: the frames of resolving a cell, working it out and running
/// its formula weigh many times those of one node. At 8, a chain of cells that the walk of names cannot see (a read through a sheet-returning
/// call, or from inside a function such as `LOCTOLOC`) ends as an error after 16 or so cells, and its frames fit 512 KiB at opt-level 0,
/// where 170 of them took 1.5 MiB.
pub const CELL_HOP_DEPTH: usize = 8;

/// The longest string a formula may build, in bytes: 1 MiB. A project rule: a string that doubles at each level of a nested call would otherwise
/// ask for more memory than there is, and a formula that long is a hostile one. Past it the result is [`BAD_ARGUMENT_ERROR`].
pub const MAX_TEXT_BYTES: usize = 1 << 20;

/// Used by the write path: a write follows at most this many `SETATREF`s.
pub const MAX_SETATREF_CHAIN: usize = 10;

/// A string that cannot convert, in `+ - * /`, the unary signs and `%`, gives this
/// error. **Project rule;** the sources disagree. To flip to About Operators' reading, make [`crate::convert`] read such a string
/// as 0 in arithmetic, in the two readers of its `Unreadable::Error`: `read_operand` and `read_double_arith`. A function's argument that
/// is the empty string is 0 (`Unreadable::EmptyIsZero`), and every other string that cannot convert is this error too.
pub const UNREADABLE_STRING_IN_ARITHMETIC: CellError = CellError::Value;

/// What the specification leaves open (and the other batches', `unknown`): an argument that cannot convert (`ABS("goat")`) and a call with
/// the wrong number of arguments. **Project rule:** `#VALUE!`, which names as the case most sources give.
pub const BAD_ARGUMENT_ERROR: CellError = CellError::Value;

/// `unknown`: a reference to a row nothing supplies
/// (`Prop.Absent`). **Project rule:** `#REF!`.
pub const MISSING_ROW_ERROR: CellError = CellError::Ref;

/// The allowances the oracle gives an `ATAN2`. A result is taken
/// as agreeing with a whole turn of difference within [`ATAN2_TURN_TOLERANCE`] when both sit within [`ATAN2_PI_TOLERANCE`] of pi in size.
pub const ATAN2_TURN_TOLERANCE: f64 = 1e-6;
/// See [`ATAN2_TURN_TOLERANCE`].
pub const ATAN2_PI_TOLERANCE: f64 = 1e-10;
/// An `ATAN2` whose operands both read under this size has no direction, and any saved value agrees.
pub const ATAN2_NO_LENGTH: f64 = 1e-6;

/// The relative tolerance of the oracle's number comparison ("Numbers agree when they differ by at most
/// 1E-9 times the larger of 1 and their sizes"). The geometry batch uses [`GEOMETRY_COMPARE_TOLERANCE`].
pub const ORACLE_TOLERANCE: f64 = 1e-9;
/// 1E-6 for the geometry functions.
pub const GEOMETRY_COMPARE_TOLERANCE: f64 = 1e-6;
/// The nine path functions agree with the application's measure of a curve within 2E-5 (it measures curves
/// numerically: 58 cells fit true arc length within 1E-5, and the spec's scan compared a curved path at 2E-5). The comparer gives a cell that calls
/// one of them this tolerance, relative to the larger of 1 and the sizes, as the others are.
pub const PATH_COMPARE_TOLERANCE: f64 = 2e-5;
/// A cached `TEXTWIDTH` agrees with the measure within 1E-4 inch, and a cached `TEXTHEIGHT` is the
/// measure plus about 5E-5 inch ("plus at most 1E-4"), so the oracle compares the two measures within 1E-4. The tolerance is
/// relative to the larger of 1 and the sizes, as the others are.
pub const TEXT_MEASURE_COMPARE_TOLERANCE: f64 = 1e-4;
/// `inferred-from-sample`, the constant a **project rule**: the small term, in
/// paper inches, that `TEXTHEIGHT` and `TEXTWIDTH` add to a text that holds a character (a line feed counts), inside the drawing scale (the sum is
/// divided by `PageScale` over `DrawingScale`, so on a scaled page the term grows with the scale). A text with no character, and a shape with no text
/// element, never gets it. The samples drift from 5.0E-5 at 9 pt by up to 7.1E-6 with the size and the line count; a constant ties with that at the
/// oracle's tolerance ([`TEXT_MEASURE_COMPARE_TOLERANCE`]). To flip: set it to 0 for no term.
pub const TEXT_MEASURE_TERM: f64 = 5e-5;

/// A project rule: whether the commit adds an `Inh` cell for an inherited result that differs from the
/// source's (true), or never creates a cell (false). The rule is true.
pub const COMMIT_ADDS_DIFFERING_INHERITED_CELLS: bool = true;

/// The most steps a chain of groups may have (project rule): a hostile file holds a chain of any depth,
/// and a carry past this gives `#VALUE!`.
pub const MAX_FRAME_STEPS: usize = 128;

/// Two lines are parallel when their directions' cross product is under this (project rule).
pub const PARALLEL_CROSS: f64 = 1e-12;

/// How far past its larger limit an angle still counts as between the limits of `GRAVITY` (project
/// rule, from 2.8E-9 observed).
pub const GRAVITY_UPPER_WINDOW: f64 = 1e-8;

/// Two products `|x| * height` and `|y| * width` are equal, for `RECTSECT`, when they differ by this much of the larger
/// (a test that needs an exact tie compares within rounding).
pub const RECTSECT_TIE: f64 = 1e-12;

/// How many bytes of text, or nodes of a formula, a step pays for. A function that reads, builds or parses text charges
/// its length at this rate before it works, so the time one step stands for is bounded by what it charged: at 64 bytes a step a byte may
/// cost up to 15 nanoseconds (a case mapping costs about 3, a parse about 7) before a step is worth more than the microsecond it was
/// measured at.
pub const UNITS_PER_STEP: usize = 64;

/// The default step budget of a whole document: 20 million. Derived in the report of Task 6 from the shapes measured there, at
/// the limits themselves: a step now stands for a bounded amount of work (a node, a cell read, 64 bytes of text), and the slowest step of any
/// shape measured costs 0.13 to 0.19 microseconds in a release build (a decode, a case mapping, a parse of 4 KB), the honest mix of masters,
/// styles, groups, `User` and `Prop` rows and strings 0.3 to 0.45 microseconds, on a machine under a load of 50 to 80 on 32 cores (read the
/// times as plus or minus 2). At a microsecond a step, a pessimistic rate for a slower machine or a browser, 20 million is 20 seconds, which
/// is the order the review asked for ("tens of seconds at most"); and it is room for about 1.3 million formula cells at 15 steps a cell.
pub const DEFAULT_STEPS: u64 = 20_000_000;

/// The default memory budget: 1 GiB, the bytes an evaluation may hold at once as [`crate::EvalCtx`] accounts them (the memo's text and
/// entries, the keys of the walks and maps, the writes of a commit not yet made). It is the sum of what grows with the document, an entry
/// counted at what it measured (about 1 KB a memoised cell), so a commit of 278,528 cells holds about 290 MiB and a million cells just fits.
/// Past it the run ends as `budget_exhausted` with `memory_exhausted`. The process memory at the limit is measured in the report of Task 6
/// It is the accounted bytes and the document's own.
pub const DEFAULT_BYTES: u64 = 1 << 30;

/// What a cell key (a sheet path and an address) in a map or a stack costs, as accounted, with the entry that holds it.
pub const KEY_BYTES: u64 = 256;

/// What an entry of the oracle's map of clock readers costs, as accounted. A large map of keys costs about 420 bytes an entry as measured
/// in the process (the entry, the key's allocations, and the table twice over while it grows), against 256 for a key in a stack: the
/// shape of 20,000 instances each reading 5,000 cells held 1.7 GB of process for an accounted 1 GiB at 256, and at this rate holds about
/// 0.65 GiB for the same 1 GiB.
pub const TAINT_ENTRY_BYTES: u64 = 640;

/// What an entry of a list of the document's names or of a level's shapes (`names.rs`, `shapes.rs`) costs, as accounted: the entry, the key
/// shared with the document, and the table while it grows.
pub const NAME_ENTRY_BYTES: u64 = 128;

/// What a memoised cell costs besides its text, as accounted.
pub const MEMO_ENTRY_BYTES: u64 = 512;

/// The longest value a report prints, in characters: a value longer than this is cut and followed by `...` and its length. A project rule:
/// one cell of a megabyte of text printed in a difference, and again as it was saved, is what ended a run at 3 GB.
pub const MAX_PRINTED_VALUE: usize = 256;

/// The most differences a report lists. Past it each further difference is counted in `differed_unlisted` and not kept. A project rule: a
/// report is read by a person or a harness, and a million differences listed in full cost hundreds of megabytes.
pub const MAX_DIFFERENCES: usize = 100_000;

/// Whether a non-finite result (infinity) agrees with a saved value only when the two are equal (`1.#INF` against `1.#INF`), where a
/// finite saved value never agrees with infinity. **Project rule** (has none): the tolerance test
/// `|a - b| <= tol * max(|a|, |b|)` is true for infinity against anything, so an oracle with no rule reports every overflow as agreed.
/// To flip: set this to `false`, and a result of infinity agrees with any saved value.
pub const NON_FINITE_AGREES_ONLY_IF_EQUAL: bool = true;
