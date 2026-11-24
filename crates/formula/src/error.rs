//! Why a formula did not parse.

use std::fmt;

/// A formula that does not parse: what went wrong and where.
///
/// A formula that fails keeps its source (`Formula::source`) and so its cached value, and counts as an error class of its
/// own (see [`ParseErrorKind::name`]): it is not a reason to refuse a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub kind: ParseErrorKind,
    /// The byte offset in the source where it went wrong. For [`ParseErrorKind::TooLong`] it is the length limit.
    pub offset: usize,
}

/// What went wrong in a parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// Nothing but white space (and maybe an `=`).
    Empty,
    /// The text is longer than [`crate::MAX_SOURCE_BYTES`].
    TooLong,
    /// Nesting or the tree is deeper than [`crate::MAX_NESTING`] or [`crate::MAX_DEPTH`].
    TooDeep,
    /// More than [`crate::MAX_NODES`] nodes, or than the budget given to [`crate::parse_with_budget`].
    TooManyNodes,
    /// The text ended where an operand, a bracket or a quote was due.
    UnexpectedEnd,
    /// An operand was due (after an operator, a comma or an opening bracket) and something else came.
    ExpectedOperand,
    /// An operator or the end was due, and something else came: `1 2`, `Width Height`.
    ExpectedOperator,
    /// `(` with no matching `)`.
    ExpectedCloseParen,
    /// In an argument list, `,` or `)` was due.
    ExpectedArgumentSeparator,
    /// A string with no closing quote.
    UnterminatedString,
    /// A name in single quotes with no closing quote, or an empty one.
    BadQuotedName,
    /// A `[index]` that is empty, too long or not closed.
    BadIndex,
    /// A `.` or `!` not followed by a name.
    BadName,
    /// A number whose text is not one.
    BadNumber,
    /// A number too large for a double, a power or a fraction beyond its range, or a zero denominator.
    NumberOutOfRange,
    /// A `#` that does not begin a colour (six hex digits). An error value has no literal, so `#REF!` is this.
    BadHash,
    /// A `{` that is not a GUID of the shape R62 gives (`{`, 8-4-4-4-12 hexadecimal digits, `}`), or is one anywhere but as the one
    /// argument of `USE`.
    BadGuid,
}

impl ParseErrorKind {
    /// A stable name for reports and counts: the error class of a formula that did not parse.
    pub fn name(self) -> &'static str {
        match self {
            ParseErrorKind::Empty => "empty",
            ParseErrorKind::TooLong => "too_long",
            ParseErrorKind::TooDeep => "too_deep",
            ParseErrorKind::TooManyNodes => "too_many_nodes",
            ParseErrorKind::UnexpectedEnd => "unexpected_end",
            ParseErrorKind::ExpectedOperand => "expected_operand",
            ParseErrorKind::ExpectedOperator => "expected_operator",
            ParseErrorKind::ExpectedCloseParen => "expected_close_paren",
            ParseErrorKind::ExpectedArgumentSeparator => "expected_argument_separator",
            ParseErrorKind::UnterminatedString => "unterminated_string",
            ParseErrorKind::BadQuotedName => "bad_quoted_name",
            ParseErrorKind::BadIndex => "bad_index",
            ParseErrorKind::BadName => "bad_name",
            ParseErrorKind::BadNumber => "bad_number",
            ParseErrorKind::NumberOutOfRange => "number_out_of_range",
            ParseErrorKind::BadHash => "bad_hash",
            ParseErrorKind::BadGuid => "bad_guid",
        }
    }

    fn message(self) -> &'static str {
        match self {
            ParseErrorKind::Empty => "the formula is empty",
            ParseErrorKind::TooLong => "the formula is too long",
            ParseErrorKind::TooDeep => "the formula is nested too deeply",
            ParseErrorKind::TooManyNodes => "the formula has too many parts",
            ParseErrorKind::UnexpectedEnd => "the formula ends too soon",
            ParseErrorKind::ExpectedOperand => "a value was expected",
            ParseErrorKind::ExpectedOperator => "an operator or the end was expected",
            ParseErrorKind::ExpectedCloseParen => "a closing bracket was expected",
            ParseErrorKind::ExpectedArgumentSeparator => "a comma or a closing bracket was expected",
            ParseErrorKind::UnterminatedString => "a string is not closed",
            ParseErrorKind::BadQuotedName => "a quoted name is empty or not closed",
            ParseErrorKind::BadIndex => "an index in square brackets is empty, too long or not closed",
            ParseErrorKind::BadName => "a name was expected",
            ParseErrorKind::BadNumber => "not a number",
            ParseErrorKind::NumberOutOfRange => "a number is out of range",
            ParseErrorKind::BadHash => "a # that is not a colour (an error value has no literal)",
            ParseErrorKind::BadGuid => "a { that is not a GUID as the one argument of USE",
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.kind.message(), self.offset)
    }
}

impl std::error::Error for ParseError {}
