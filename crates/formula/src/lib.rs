//! The ShapeSheet formula language: a parser and a printer, and nothing that evaluates.
//!
//! [`parse`] reads the text of a cell's formula into an [`Expr`]; [`print`] writes a tree back as text. The evaluator is `calc`'s: this
//! crate does not look a name up, convert a unit or choose a result.
//!
//! **The source is kept.** A [`Formula`] holds the text exactly as given, whether it parses or not, and `print` is not a way
//! to save one (a document keeps the source verbatim, and so keeps a formula this parser cannot read). `print` is there to
//! check a parse and to word a message. A formula that does not parse is an error class of its own
//! ([`ParseErrorKind::name`]) and not a reason to refuse a file. The file markers `No Formula` and `Inh`
//! are the reader's to tell apart before it calls `parse`: `Inh` alone is a valid reference.
//!
//! **Hostile text.** A formula can be as hostile as the file it came from. The parser reads at most [`MAX_SOURCE_BYTES`], nests
//! at most [`MAX_NESTING`] brackets, signs and calls, builds a tree at most [`MAX_DEPTH`] deep (a long chain `a+a+a+...` counts,
//! one level a term) and at most [`MAX_NODES`] nodes, and any text gives a tree or a [`ParseError`]. Code that walks a tree may
//! recurse to [`MAX_DEPTH`].
//!
//! **What it accepts**, as the spec says. Function names are matched without regard to case, so the tree keeps them as written
//! and [`Call::is`] compares them. Cell, section, row and sheet names keep their case. A name the parser does not
//! know is still a call, including names with a leading underscore. An empty argument slot keeps its place. The
//! forms from the reference pages only (`3[cm,D]`, `5 1/2`, `SQ. IN.`) are read too. Not read: `;` between arguments (R49
//! says files hold the universal form).
//!
//! **Currency.** A currency literal is a number with one of the 192 codes of after it, with a blank
//! between or none, and in capitals: `12.5EUR` and `12.5 EUR` are one, and `12.5ZZZ` and `12.5eur`
//! are a number and then a name, which is no operator. A currency prints with no blank (`12.5EUR`), which reads back as the same
//! tree. [`CURRENCIES`] holds the table and [`currency_by_code`] and [`currency_by_id`] look an entry up, for the evaluator of `CY`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod ast;
mod currency;
mod error;
mod parser;
mod print;

use std::ops::Range;
use std::sync::Arc;

pub use ast::{BinaryOp, Call, CellRef, Expr, ImplicitMeasure, Index, Number, PERCENT_FUNCTION, Qualifier, Segment, UnaryOp};
pub use currency::{CURRENCIES, Currency, currency_by_code, currency_by_id};
pub use error::{ParseError, ParseErrorKind};

/// The longest formula read, in bytes. The spec gives no limit; 1 MiB is far beyond what files hold
/// and keeps the work and the tree of one cell bounded.
pub const MAX_SOURCE_BYTES: usize = 1 << 20;
/// How deep brackets, signs and call argument lists may nest while parsing: the recursion of the parser. (A run of `%` is a loop,
/// not a recursion; it adds a level to the tree, which `MAX_DEPTH` limits.)
pub const MAX_NESTING: usize = 100;
/// How deep the tree may be, a leaf being 1. A left-leaning chain `a+a+a+...` adds a level a term, so this is also the
/// longest chain. Bounds the recursion of everything that walks a tree, `Drop` and [`print`] among them.
pub const MAX_DEPTH: usize = 512;
/// The most nodes a tree may hold: every expression, every path segment and every empty argument slot counts one, so a call counts
/// itself and its name, and then its arguments. This is per formula. A tree costs memory by the node, counted as bytes requested
/// from the allocator in the review of this crate: 72 a node for `F(A,A,...)` (130,002 nodes for 130 KB of text, about 9.4 million
/// bytes) and 128 a node for `F(Q!A,...)`, a name with a qualifier. (A name is a segment, an `Arc`, a `Box` and a `Vec`.) So the
/// cells of a document need a total of their own: [`parse_with_budget`] and [`Expr::node_count`] are how a reader keeps one.
pub const MAX_NODES: usize = 1 << 17;

/// A formula: its text, and what the text parsed to.
#[derive(Clone, Debug, PartialEq)]
pub struct Formula {
    /// The text exactly as given. Never changed, never normalised, whatever the parse did.
    pub source: Arc<str>,
    /// The tree, or why there is none.
    pub ast: Result<Expr, ParseError>,
}

impl Formula {
    /// The nodes of the tree, as [`Expr::node_count`] counts them, or 0 if the text did not parse. What a reader subtracts from the
    /// budget of a document after [`parse_with_budget`].
    pub fn node_count(&self) -> usize {
        self.ast.as_ref().map_or(0, Expr::node_count)
    }

    /// Whether the text parsed.
    pub fn is_parsed(&self) -> bool {
        self.ast.is_ok()
    }
}

/// Parses a formula, as a file holds it (`F` of a `Cell`) or as typed: one leading `=` is allowed and ignored
/// The source is kept as given, and an error does not lose it.
pub fn parse(source: &str) -> Formula {
    parse_with_budget(source, MAX_NODES)
}

/// [`parse`] with a budget: a formula of more than `max_nodes` nodes (counted as [`MAX_NODES`] is) is `TooManyNodes`, and the tree
/// is not built past it. The limits of the crate are per formula; a document of many cells keeps a total of its own by giving each
/// parse what is left of it and taking off [`Formula::node_count`] after. A budget cannot raise `MAX_NODES`, and `0` refuses every
/// formula. The source is kept as given, as in `parse`.
pub fn parse_with_budget(source: &str, max_nodes: usize) -> Formula {
    Formula { source: Arc::from(source), ast: parser::parse(source, max_nodes) }
}

/// Where one cell reference sits in the text of a formula ([`reference_spans`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RefSpan {
    /// The first byte of the reference: its first qualifier, or its first name.
    pub start: usize,
    /// The byte after its last name.
    pub end: usize,
    /// The reference as the parser made it from those bytes.
    pub reference: CellRef,
    /// The segments of its path (not of its qualifiers), each with where it sits. As many as `reference.path` holds.
    pub segments: Vec<SegmentSpan>,
}

/// Where one name of a path sits, and the row number in brackets after it, if it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentSpan {
    /// The bytes of the name as written, with its quotes if it is quoted.
    pub name: Range<usize>,
    /// The bytes of the digits of an `[index]` that is a row number (`[2]`): none for a name with no index, and for one that holds a name (`[Site-2]`).
    pub index: Option<Range<usize>>,
}

/// Parses `source` as [`parse`] does (with its limits, and one leading `=` allowed) and says where each cell reference sits in it, in the order the references
/// begin: a reference inside the arguments of a call that qualifies another comes after the one it is inside. A call, a `TRUE`, a string and a number are none.
/// It is for a program that rewrites the numbers inside a formula's references and must leave every other byte of the formula as it was: the text a span
/// covers, read alone, is the reference the parser made of it.
pub fn reference_spans(source: &str) -> Result<Vec<RefSpan>, ParseError> {
    parser::parse_spans(source)
}

/// Writes a tree as formula text, in a form that parses back to the same tree: names and numbers as the tree holds them,
/// operators as symbols (the word forms `GT` and the rest come back as `_GT_`, and the function spellings stay calls), one
/// space around a binary operator, and brackets where the ranks of need them.
///
/// It is for checking a parse and for messages, not for saving (see the crate documentation). A tree this crate did not
/// parse can hold what no text gives (a negative literal, a NaN, one empty argument slot, which prints as none); its print is not
/// promised to parse back. A tree deeper than [`MAX_DEPTH`] prints `...` below that depth.
///
/// **A length caveat.** What `print` writes can be longer than the text the tree came from: a blank each side of a binary operator,
/// after a comma and before a unit, a number written out in full below 1e16. A formula whose text is close to
/// [`MAX_SOURCE_BYTES`] can print past it, and then the print does not parse (`TooLong`). A message does not need it to; a caller
/// that wants the round trip checks the length first.
pub fn print(expr: &Expr) -> String {
    print::print(expr)
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&print(self))
    }
}
