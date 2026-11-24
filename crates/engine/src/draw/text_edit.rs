//! The text of a shape: a range of its characters replaced, and nothing else of it touched (of the M3 review).
//!
//! A shape's text is kept in parts (`TextPart`): the characters, the `cp`, `pp` and `tp` markers that start a run, a paragraph and a tab list,
//! and the `fld` fields with the text they display. The markers and the rows they name must stay in step, so an edit of the text changes
//! the characters and **no marker and no field**: typing over `cd` to give `ce` changes the character data and nothing else.
//!
//! **The editable text** is the characters of the shape, the parts that are `Chars` put end to end; a field is not in it (it is not typed over:
//! its result is the field's), and neither is a marker. A **range** `start..end` is counted in characters (Unicode scalar values, not bytes) of
//! the editable text; `start == end` is a point. [`set_text_range`] replaces the characters of a range with new ones:
//!
//! * every marker and every field stays, in its place, whatever the range covers (ours: no specification says what typing over a
//!   selection that holds a field does, and keeping the field is the edit that loses nothing);
//! * the new characters join **the run of the first character replaced**; at a point they join the run of the character before it (so characters
//!   typed at the end of a run are that run's, and not the next one's), and at the start of the text the run of the first character. A text with
//!   no characters yet takes them after its leading markers, ahead of any field;
//! * a part left with no characters is taken out, its markers staying, so the next characters typed after the marker are the run's own.

use std::sync::Arc;

use diacraft_doc::{Document, Owner, Text, TextPart};

use super::settle::settle_text;
use super::{MAX_TEXT_CHARS, Problem, require_shape, shape_mut};

/// What a shape's text is for an editor: its characters, and where its fields sit among them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextView {
    /// The editable text: the characters of the shape, fields and markers left out.
    pub chars: String,
    /// The fields, in order.
    pub fields: Vec<TextField>,
}

/// A field of a shape's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextField {
    /// How many characters of [`TextView::chars`] come before the field.
    pub at: usize,
    /// The `IX` of its `Field` row.
    pub ix: u32,
    /// What it displays.
    pub text: String,
}

/// The text of shape `id` on a page as an editor shows it ([`TextView`]); empty for a shape with no text. `None` for a page or shape that is
/// not there.
pub fn text_view(doc: &Document, page: u32, id: u32) -> Option<TextView> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    let shape = doc.shape(&at)?;
    let mut view = TextView { chars: String::new(), fields: Vec::new() };
    let mut count = 0_usize;
    for part in shape.text.iter().flat_map(|t| t.parts.iter()) {
        match part {
            TextPart::Chars(s) => {
                view.chars.push_str(s);
                count = count.saturating_add(s.chars().count());
            }
            TextPart::Fld { ix, text } => view.fields.push(TextField { at: count, ix: *ix, text: text.to_string() }),
            _ => {}
        }
    }
    Some(view)
}

/// The characters XML cannot hold are refused: the controls other than a line break and a tab (the C0 range, DEL and the C1 range), and U+FFFE
/// and U+FFFF, which no XML document may contain (a save would otherwise fail naming only the page). Too long a text is refused too.
pub(super) fn check_text(text: &str) -> Result<(), Problem> {
    if text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS {
        return Err(Problem::bad(format!("`text` must be at most {MAX_TEXT_CHARS} characters")));
    }
    if text.chars().any(|c| (c.is_control() && !matches!(c, '\n' | '\t')) || matches!(c, '\u{fffe}' | '\u{ffff}')) {
        return Err(Problem::bad("`text` must not contain control characters other than line breaks and tabs, nor U+FFFE or U+FFFF"));
    }
    Ok(())
}

/// Replaces the text of a shape. Empty text takes the text away. Text is kept as given, line breaks included; other control characters are
/// refused (see [`check_text`]), and so is more than [`MAX_TEXT_CHARS`] characters.
///
/// **The whole text goes**: the one run of `text` is all that is left, and the markers and fields with it. The exception is a text the shape
/// already shows (its characters with the results of its fields in place): then nothing is touched, since replacing it would only lose the runs
/// and fields of text that did not change. An editor that knows what changed uses [`set_text_range`]. → whether the text changed.
pub fn set_text(doc: &mut Document, page: u32, id: u32, text: &str) -> Result<bool, Problem> {
    check_text(text)?;
    let at = require_shape(doc, page, id)?;
    let unchanged = {
        let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
        match shape.text.as_ref() {
            None => text.is_empty(),
            Some(t) => !text.is_empty() && displayed(&t.parts) == text,
        }
    };
    if unchanged {
        return Ok(false);
    }
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    // The text's ODF data goes with it: the file's paragraphs as it held them, which the writer matches to the new ones by content.
    let ext = shape.text.as_ref().map(|t| t.ext.clone()).unwrap_or_default();
    shape.text = if text.is_empty() { None } else { Some(Arc::new(Text { parts: vec![TextPart::Chars(Arc::from(text))], ext })) };
    settle_text(doc, page, &at)?;
    Ok(true)
}

/// What a text shows: the characters and the results of the fields, in order.
fn displayed(parts: &[TextPart]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            TextPart::Chars(s) => out.push_str(s),
            TextPart::Fld { text, .. } => out.push_str(text),
            _ => {}
        }
    }
    out
}

/// Replaces the characters `start..end` of the editable text of shape `id` with `text` and nothing else (the module says how). `start..end` is
/// counted in characters, `end` is not in the range, and `start == end` inserts. A range the text does not have, text that is not text
/// ([`set_text`]) and a result of more than [`MAX_TEXT_CHARS`] characters are refused, and the document is as it was. → whether the text changed
/// (a point with nothing to put in it changes nothing).
pub fn set_text_range(doc: &mut Document, page: u32, id: u32, start: usize, end: usize, text: &str) -> Result<bool, Problem> {
    check_text(text)?;
    let at = require_shape(doc, page, id)?;
    let parts = {
        let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
        let old: &[TextPart] = shape.text.as_ref().map_or(&[], |t| t.parts.as_slice());
        match replaced(old, start, end, text)? {
            Some(parts) => parts,
            // Nothing is asked: nothing changes.
            None => return Ok(false),
        }
    };
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    // As `set_text`: the text's ODF data goes with it, for the writer to match the file's paragraphs to the new ones by content.
    let ext = shape.text.as_ref().map(|t| t.ext.clone()).unwrap_or_default();
    shape.text = if parts.is_empty() { None } else { Some(Arc::new(Text { parts, ext })) };
    settle_text(doc, page, &at)?;
    Ok(true)
}

/// How many characters a part holds that can be edited.
fn chars_in(part: &TextPart) -> usize {
    match part {
        TextPart::Chars(s) => s.chars().count(),
        _ => 0,
    }
}

/// `s` cut after `k` characters.
fn cut(s: &str, k: usize) -> (&str, &str) {
    let at = s.char_indices().nth(k).map_or(s.len(), |(byte, _)| byte);
    s.split_at_checked(at).unwrap_or((s, ""))
}

/// The parts that `parts` become when the characters `start..end` are replaced by `text`; `None` when that changes nothing.
fn replaced(parts: &[TextPart], start: usize, end: usize, text: &str) -> Result<Option<Vec<TextPart>>, Problem> {
    let total: usize = parts.iter().map(chars_in).fold(0, usize::saturating_add);
    if start > end || end > total {
        return Err(Problem::bad(format!(
            "the range {start} to {end} is not within the text, which has {total} characters (fields are not counted); `start` must not be more than `end`"
        )));
    }
    let added = text.chars().count();
    if total.saturating_sub(end - start).saturating_add(added) > MAX_TEXT_CHARS {
        return Err(Problem::bad(format!("the text would be more than {MAX_TEXT_CHARS} characters")));
    }
    if start == end && added == 0 {
        return Ok(None);
    }
    if total == 0 {
        // No characters to join: the new ones follow the markers that start the text, ahead of a field or anything else.
        let mut out = parts.to_vec();
        let at = out.iter().position(|p| !matches!(p, TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_))).unwrap_or(out.len());
        out.insert(at, TextPart::Chars(Arc::from(text)));
        return Ok(Some(out));
    }
    // The character whose run takes the new ones: the first replaced, else the one before the point, else (at the start) the first.
    let home = if start < end || start == 0 { start } else { start - 1 };
    let mut out: Vec<TextPart> = Vec::with_capacity(parts.len() + 1);
    let mut pos = 0_usize;
    for part in parts {
        let TextPart::Chars(s) = part else {
            out.push(part.clone());
            continue;
        };
        let n = s.chars().count();
        let (from, to) = (start.saturating_sub(pos).min(n), end.saturating_sub(pos).min(n));
        let is_home = (pos..pos.saturating_add(n)).contains(&home);
        if is_home || from < to {
            let (head, rest) = cut(s, from);
            let (_, tail) = cut(rest, to - from);
            let mut kept = String::with_capacity(head.len().saturating_add(tail.len()).saturating_add(if is_home { text.len() } else { 0 }));
            kept.push_str(head);
            if is_home {
                kept.push_str(text);
            }
            kept.push_str(tail);
            if !kept.is_empty() {
                out.push(TextPart::Chars(Arc::from(kept)));
            }
        } else {
            out.push(part.clone());
        }
        pos = pos.saturating_add(n);
    }
    Ok(Some(out))
}
