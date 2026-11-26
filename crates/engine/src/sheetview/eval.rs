//! The dry run (`sheet.evaluate`, design 7.3): a text worked out on a sheet as the formula of a cell would be, with nothing written.
//!
//! The text is cut as a write cuts it (step 1): white space at its ends goes, and one leading `=`. A parse error is reported with its
//! kind and its **byte offset in the text as given**, so that a window can mark the fault where it was typed, and in words that name the character it is at. The work is held
//! to [`EVALUATE_STEPS`](super::EVALUATE_STEPS) steps of the evaluator, counted and never timed; past them the answer says `budget_exhausted`.

use diacraft_calc::{Budget, EvalOptions, EvalResult, evaluate_text_with};
use diacraft_doc::{CellAddr, Document, SheetRef};
use diacraft_formula::{ParseError, ParseErrorKind};
use serde_json::{Value, json};

use super::EVALUATE_STEPS;
use super::text::{self, CellValue, Typed};

/// A fault in a typed text: what is wrong, where, and in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    /// The parser's stable name of the error (`unexpected_end`, `expected_operand`).
    pub kind: &'static str,
    /// The byte offset in the text as given.
    pub offset: usize,
    /// The sentence a person reads: "a value is due at character 3".
    pub words: String,
}

impl Fault {
    pub fn json(&self) -> Value {
        json!({"kind": self.kind, "offset": self.offset, "words": self.words})
    }
}

/// What a dry run came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Dry {
    pub typed: Typed,
    /// The error the text works out to (`#DIV/0!`): a value, not a refusal.
    pub error: Option<String>,
    pub parse_error: Option<Fault>,
    pub budget_exhausted: bool,
    /// The evaluator could work the text out: not a function it does not have, and not an action cell.
    pub worked_out: bool,
    pub steps: u64,
}

/// The text as a write reads it: cut of white space at both ends and of one leading `=`, and the bytes cut at the front.
pub fn cut_text(text: &str) -> (&str, usize) {
    let trimmed = text.trim();
    let body = trimmed.strip_prefix('=').unwrap_or(trimmed);
    let front = text.len().saturating_sub(text.trim_start().len()).saturating_add(trimmed.len().saturating_sub(body.len()));
    (body, front)
}

/// The sentence for a parse error at `offset` of `text`.
pub fn fault_words(kind: ParseErrorKind, text: &str, offset: usize) -> String {
    let at = text.get(..offset).map_or_else(|| text.chars().count(), |before| before.chars().count()).saturating_add(1);
    match kind {
        ParseErrorKind::Empty => "the formula is empty".to_owned(),
        ParseErrorKind::TooLong => format!("the formula is longer than {} bytes", diacraft_formula::MAX_SOURCE_BYTES),
        ParseErrorKind::TooDeep => format!("the formula is nested too deeply at character {at}"),
        ParseErrorKind::TooManyNodes => "the formula has too many parts".to_owned(),
        ParseErrorKind::UnexpectedEnd => format!("the formula ends too soon: something is due at character {at}"),
        ParseErrorKind::ExpectedOperand => format!("a value is due at character {at}"),
        ParseErrorKind::ExpectedOperator => format!("an operator or the end is due at character {at}"),
        ParseErrorKind::ExpectedCloseParen => format!("a closing bracket is due at character {at}"),
        ParseErrorKind::ExpectedArgumentSeparator => format!("a comma or a closing bracket is due at character {at}"),
        ParseErrorKind::UnterminatedString => format!("a string is not closed: its quote at character {at} has no partner"),
        ParseErrorKind::BadQuotedName => format!("a quoted name is empty or not closed at character {at}"),
        ParseErrorKind::BadIndex => format!("an index in square brackets is empty, too long or not closed at character {at}"),
        ParseErrorKind::BadName => format!("a name is due at character {at}"),
        ParseErrorKind::BadNumber => format!("not a number at character {at}"),
        ParseErrorKind::NumberOutOfRange => format!("a number is out of range at character {at}"),
        ParseErrorKind::BadHash => format!("a # that is not a colour at character {at} (an error value has no literal)"),
        ParseErrorKind::BadGuid => format!("a {{ that is not a GUID as the one argument of USE at character {at}"),
    }
}

/// The fault of a parse error in the text `given`, whose cut body begins `front` bytes in.
pub fn fault_of(error: &ParseError, given: &str, front: usize) -> Fault {
    let offset = error.offset.saturating_add(front).min(given.len());
    Fault { kind: error.kind.name(), offset, words: fault_words(error.kind, given, offset) }
}

/// Works `given` out as the formula of `at` (a cell, whose unit the result is fitted to) on `sheet`, or as one of the sheet's own when `at` is `None`. Nothing is written.
pub fn evaluate(doc: &Document, sheet: &SheetRef, at: Option<&CellAddr>, given: &str, options: &EvalOptions) -> Dry {
    let (body, front) = cut_text(given);
    let done = evaluate_text_with(doc, sheet, at, body, options, &Budget::with_steps(EVALUATE_STEPS));
    let nothing = || Typed { value: CellValue::None, unit: None, shown: String::new(), literal: String::new() };
    let mut dry =
        Dry { typed: nothing(), error: None, parse_error: None, budget_exhausted: done.budget_exhausted, worked_out: true, steps: done.steps };
    match done.result {
        EvalResult::ParseError(e) => {
            dry.parse_error = Some(fault_of(&e, given, front));
            dry.worked_out = false;
        }
        EvalResult::ActionCell => dry.worked_out = false,
        // A run the budget stopped has no value to give: the answer says so and the error it would read is the budget's, not the formula's.
        EvalResult::Value(_) if done.budget_exhausted => dry.worked_out = false,
        EvalResult::Value(v) => {
            dry.error = text::error_of(&v).map(str::to_owned);
            dry.worked_out = !matches!(v, diacraft_calc::Value::Unsupported(_));
            dry.typed = if dry.error.is_some() {
                Typed { shown: v.error().map_or_else(String::new, |e| e.text().to_owned()), ..nothing() }
            } else {
                text::typed_calc(&v)
            };
        }
    }
    dry
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_text_is_cut_as_a_write_cuts_it_and_the_front_is_counted() {
        assert_eq!(cut_text("  =1+2 "), ("1+2", 3));
        assert_eq!(cut_text("=1"), ("1", 1));
        assert_eq!(cut_text("1"), ("1", 0));
        assert_eq!(cut_text("   "), ("", 3));
        assert_eq!(cut_text("==1"), ("=1", 1), "only one `=` goes");
        assert_eq!(cut_text("= 1"), (" 1", 1));
        assert_eq!(cut_text(""), ("", 0));
    }

    #[test]
    fn the_words_of_a_fault_name_the_character_and_never_cut_a_character() {
        assert_eq!(fault_words(ParseErrorKind::ExpectedOperand, "1+*2", 2), "a value is due at character 3");
        assert_eq!(fault_words(ParseErrorKind::UnexpectedEnd, "é+", 3), "the formula ends too soon: something is due at character 3");
        // An offset inside a character, or past the end, is read as the end.
        assert!(fault_words(ParseErrorKind::BadName, "é", 1).contains("character 2"));
        assert!(fault_words(ParseErrorKind::BadName, "ab", 99).contains("character 3"));
    }
}
