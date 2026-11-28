//! Where the index of a `Pages[...]` qualifier sits in a formula's text.
//!
//! A formula names another page by `Pages[<page>]!` before a reference or a call (`Pages[Site-2]!Sheet.9!EndY`). The parser keeps no span for the
//! index of a qualifier, so a program that must change the name a formula gives a page (a writer that renames the page, and must keep the formulas that name it) cannot ask it. [`page_index_spans`]
//! reads the text the way the parser reads a qualifier's index (one pass, no tree) and says where the bytes between the brackets are and what they name, so that the caller replaces those
//! bytes and keeps every other byte of the formula.

use std::ops::Range;

use crate::Index;
use crate::parser::{MAX_INDEX_BYTES, MAX_NAME_BYTES};

/// The word before the bracket, in any case.
const WORD: &[u8] = b"pages";

/// The index of a `Pages[...]` qualifier in the text of a formula: the bytes between the brackets, and what they name.
#[derive(Clone, Debug, PartialEq)]
pub struct PageIndexSpan {
    /// The bytes between `[` and `]`, blanks and quotes included: what a rewrite replaces.
    pub inside: Range<usize>,
    /// The page the bytes name, as the parser reads them: a whole number that fits is a number, a name in quotes is a name (`['2']` is the name 2), anything else is a name without the
    /// blanks at its ends.
    pub index: Index,
}

/// Every `Pages[...]` qualifier of `source`, in the order of the text. **Not found:** text in a string or in a quoted name, the word inside a longer name or after a dot (`MyPages[1]`,
/// `Prop.Pages[2]`), a bracket that is not closed, an empty index, an unquoted index of more than 256 bytes or a quoted one of more than 1,024, and the word with a blank before its bracket
/// (the parser reads no qualifier there). One pass over the text: a scan for a closing bracket or quote stops at the longest index the parser reads, so no more is found than the text holds
/// and the work is linear in its length.
pub fn page_index_spans(source: &str) -> Vec<PageIndexSpan> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let (mut at, mut in_text, mut in_name) = (0_usize, false, false);
    while let Some(&b) = bytes.get(at) {
        match b {
            b'"' if !in_name => in_text = !in_text,
            b'\'' if !in_text => in_name = !in_name,
            b'P' | b'p' if !in_text && !in_name && starts_qualifier(bytes, at) => {
                let open = at + WORD.len();
                if bytes.get(at..open).is_some_and(|word| word.eq_ignore_ascii_case(WORD))
                    && bytes.get(open) == Some(&b'[')
                    && let Some((span, after)) = index_at(source, open)
                {
                    out.push(span);
                    at = after;
                    continue;
                }
            }
            _ => {}
        }
        at += 1;
    }
    out
}

/// Whether a word that starts at `at` starts a qualifier: nothing before it joins it to a name or a path.
fn starts_qualifier(bytes: &[u8], at: usize) -> bool {
    match at.checked_sub(1).and_then(|before| bytes.get(before)) {
        None => true,
        // A byte of a longer name, a letter of another script included, a path, an index or a quote.
        Some(b) => !(b.is_ascii_alphanumeric() || *b >= 0x80 || matches!(b, b'_' | b'.' | b'[' | b']' | b'\'' | b'"')),
    }
}

/// The index whose `[` is at `open`, and the place after its `]`: as the parser reads it.
fn index_at(source: &str, open: usize) -> Option<(PageIndexSpan, usize)> {
    let inner = open + 1;
    let rest = source.get(inner..)?;
    let blanks = rest.len() - rest.trim_start().len();
    let start = inner + blanks;
    if source.as_bytes().get(start) == Some(&b'\'') {
        let (name, after) = quoted_name(source, start)?;
        let tail = source.get(after..)?;
        let close = after + (tail.len() - tail.trim_start().len());
        if source.as_bytes().get(close) != Some(&b']') {
            return None;
        }
        return Some((PageIndexSpan { inside: inner..close, index: Index::Name(name.into()) }, close + 1));
    }
    let text = source.get(start..)?;
    let end = text.bytes().take(MAX_INDEX_BYTES + 1).position(|b| b == b']')?;
    let raw = text.get(..end)?.trim();
    if raw.is_empty() {
        return None;
    }
    let index = match raw.bytes().all(|b| b.is_ascii_digit()).then(|| raw.parse::<u32>().ok()).flatten() {
        Some(number) => Index::Number(number),
        None => Index::Name(raw.into()),
    };
    Some((PageIndexSpan { inside: inner..start + end, index }, start + end + 1))
}

/// The name in single quotes that starts at `start` (a quote inside it doubled), and the place after its closing quote. Not empty, and no longer than the parser reads.
fn quoted_name(source: &str, start: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut at = start + 1;
    loop {
        let rest = source.get(at..)?;
        let end = rest.bytes().take(MAX_NAME_BYTES + 1).position(|b| b == b'\'')?;
        name.push_str(rest.get(..end)?);
        at += end + 1;
        if source.as_bytes().get(at) == Some(&b'\'') {
            name.push('\'');
            at += 1;
        } else {
            break;
        }
        if name.len() > MAX_NAME_BYTES {
            return None;
        }
    }
    (!name.is_empty() && name.len() <= MAX_NAME_BYTES).then_some((name, at))
}
