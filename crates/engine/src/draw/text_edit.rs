//! The text of a shape: a range of its characters replaced, and nothing else of it touched (of the M3 review).
//!
//! A shape's text is kept in parts (`TextPart`): the characters, the `cp`, `pp` and `tp` markers that start a run, a paragraph and a tab list,
//! and the `fld` fields with the text they display. The markers and the rows they name must stay in step, so an edit of the text changes
//! the characters and **no marker and no field**: typing over `cd` to give `ce` changes the character data and nothing else.
//!
//! **The editable text** is the characters of the text the shape shows, the parts that are `Chars` put end to end; a field is not in it (it is not typed over:
//! its result is the field's), and neither is a marker. **The text a shape shows is its own, and when it holds none its master's** ([`shown_parts`]): a UML compartment that has
//! not been typed in holds no text and shows `+ attribute : Type`, and those are its characters. A **range** `start..end` is counted in characters (Unicode scalar values, not bytes)
//! of the editable text; `start == end` is a point. [`set_text_range`] replaces the characters of a range with new ones, and **a shape that shows its master's text takes it as its own
//! with the change made**, markers and fields in their places, and goes on inheriting while nothing is changed:
//!
//! * every marker and every field stays, in its place, whatever the range covers (ours: no specification says what typing over a
//!   selection that holds a field does, and keeping the field is the edit that loses nothing);
//! * the new characters join **the run of the first character replaced**; at a point they join the run of the character before it (so characters
//!   typed at the end of a run are that run's, and not the next one's), and at the start of the text the run of the first character. A text with
//!   no characters yet takes them after its leading markers, ahead of any field;
//! * a part left with no characters is taken out, its markers staying, so the next characters typed after the marker are the run's own;
//! * a text left with nothing in it is no `Text`, **unless the shape shows its master's text** (a member of a group master, a shape of a master with a label): then it is a `Text` of its own with
//!   nothing in it, which shows nothing, and not the master's placeholder coming back ([`kept_text`]).

use std::sync::Arc;

use diacraft_doc::{Document, Ext, OdfText, Owner, Resolver, ShapeRef, Text, TextPart};

use super::settle::{route_recalculated, settle_text};
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

/// The text shape `id` on a page **holds itself** ([`TextView`]): its own characters and fields, empty for a shape with no `Text` (one that shows its master's holds none: see [`shown_text_view`]).
/// `None` for a page or shape that is not there.
pub fn text_view(doc: &Document, page: u32, id: u32) -> Option<TextView> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    let shape = doc.shape(&at)?;
    Some(view_of(shape.text.iter().flat_map(|t| t.parts.iter())))
}

/// The text of shape `id` on a page **as an editor shows it** ([`TextView`]): the text the shape shows, its own and when it holds none its master's ([`shown_parts`]). The text box of the
/// window opens on these characters, and a range of `shape.set_text`, `text.insert` and `text.format` counts them. `None` for a page or shape that is not there.
pub fn shown_text_view(doc: &Document, page: u32, id: u32) -> Option<TextView> {
    let at = doc.find_shape(Owner::Page(page), id)?;
    doc.shape(&at)?;
    Some(view_of(shown_parts(doc, &at).iter()))
}

/// The characters of `parts` and where their fields sit among them.
fn view_of<'a>(parts: impl Iterator<Item = &'a TextPart>) -> TextView {
    let mut view = TextView { chars: String::new(), fields: Vec::new() };
    let mut count = 0_usize;
    for part in parts {
        match part {
            TextPart::Chars(s) => {
                view.chars.push_str(s);
                count = count.saturating_add(s.chars().count());
            }
            TextPart::Fld { ix, text } => view.fields.push(TextField { at: count, ix: *ix, text: text.to_string() }),
            _ => {}
        }
    }
    view
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

/// The parts of the text the master of the shape `at` gives it, when the shape is an instance, or a member of one, whose master shape has text with something in it (a character, or a field,
/// which shows its result). `None` for any other shape, and for one whose master shape has no text or only markers. This is what the shape shows while it holds no `Text` of its own
/// (a shape with no `Text` shows its master's; a shape with a `Text` of its own that is empty shows no text, and the master's plays no part).
fn master_parts<'a>(doc: &'a Document, at: &ShapeRef) -> Option<&'a [TextPart]> {
    let res = Resolver::new(doc);
    let Ok(Some(step)) = res.master_step(at) else { return None };
    let master = doc.shape(&step.shape).and_then(|m| m.text.as_ref())?;
    let something = master.parts.iter().any(|part| match part {
        TextPart::Chars(s) => !s.is_empty(),
        TextPart::Fld { .. } => true,
        _ => false,
    });
    something.then_some(master.parts.as_slice())
}

/// Whether the shape `at` shows its master's text: see [`master_parts`]. (Whether it holds a `Text` of its own is for the caller to say.)
fn inherits_text(doc: &Document, at: &ShapeRef) -> bool {
    master_parts(doc, at).is_some()
}

/// **The parts of the text the shape `at` shows, which is the text its editors open on and its ranges count in**: its own `Text`, and when it holds none the text its master gives it
/// ([`master_parts`]): the characters, the markers that start a run, a paragraph and a tab list, and the fields. A shape that holds an empty `Text` of its own shows none, and a
/// shape with no master text to show has no parts. The text a shape does not hold itself is not changed by reading it: an edit that changes it writes the shape's own `Text`, made of these parts with
/// the change in them ([`set_text_range`]), so the runs of the master's text go on being its runs: the rows the markers name are found by inheritance, as they were.
pub(crate) fn shown_parts<'a>(doc: &'a Document, at: &ShapeRef) -> &'a [TextPart] {
    match doc.shape(at).and_then(|shape| shape.text.as_ref()) {
        Some(own) => own.parts.as_slice(),
        None => master_parts(doc, at).unwrap_or(&[]),
    }
}

/// The `Text` a shape holds when its text is `parts` (what its characters, markers and fields come to): the text of those parts, or, when there are none, **no `Text` at all, unless the shape has a
/// master's text to override**, and then a `Text` of its own with nothing in it. A shape with no `Text` shows its master's (a member of a group master holds its master's
/// placeholder until it is typed over), so taking the text of such a shape away by leaving it with none brought the placeholder back: a UML Interface whose attributes were set to the empty
/// text drew `+ attribute : Type` again. An empty `Text` of its own is what a file says for "no text here" (`<Text/>`), in each format, and the renderer, the measure and the writers read it so.
/// `ext` is the text's ODF data, which goes with it ([`set_text`]).
pub(crate) fn kept_text(doc: &Document, at: &ShapeRef, parts: Vec<TextPart>, ext: Ext<OdfText>) -> Option<Arc<Text>> {
    if parts.is_empty() && !inherits_text(doc, at) {
        return None;
    }
    Some(Arc::new(Text { parts, ext }))
}

/// Replaces the text of a shape. Empty text takes the text away. Text is kept as given, line breaks included; other control characters are
/// refused (see [`check_text`]), and so is more than [`MAX_TEXT_CHARS`] characters.
///
/// **The whole text goes**: the one run of `text` is all that is left, and the markers and fields with it. The exception is a text the shape
/// already shows (its characters with the results of its fields in place): then nothing is touched, since replacing it would only lose the runs
/// and fields of text that did not change. An editor that knows what changed uses [`set_text_range`]. → whether the text changed.
///
/// **Empty text on a shape that shows its master's text** (a member of a group master, a shape of a master that has a label) leaves the shape a `Text` of its own with nothing in it, so that it
/// shows nothing and not the master's placeholder (`kept_text`); on a shape that has no master text to show, it leaves none, as it always did.
pub fn set_text(doc: &mut Document, page: u32, id: u32, text: &str) -> Result<bool, Problem> {
    check_text(text)?;
    let at = require_shape(doc, page, id)?;
    let hold_empty = text.is_empty() && inherits_text(doc, &at);
    let unchanged = {
        let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
        match shape.text.as_ref() {
            None => text.is_empty() && !hold_empty,
            // An empty text is held as it is when the shape has a master's text to override and holds an empty one already, and is no text for any other shape.
            Some(t) if text.is_empty() => hold_empty && t.parts.is_empty(),
            Some(t) => displayed(&t.parts) == text,
        }
    };
    if unchanged {
        return Ok(false);
    }
    let was = doc.clone();
    let parts = if text.is_empty() { Vec::new() } else { vec![TextPart::Chars(Arc::from(text))] };
    // The text's ODF data goes with it: the file's paragraphs as it held them, which the writer matches to the new ones by content.
    let ext = doc.shape(&at).and_then(|shape| shape.text.as_ref()).map(|t| t.ext.clone()).unwrap_or_default();
    let kept = kept_text(doc, &at, parts, ext);
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    shape.text = kept;
    settle_text(doc, page, &at)?;
    // A text that makes a compartment taller resizes its shape by a formula, and the connectors glued to the shape follow it.
    route_recalculated(doc, &was, page)?;
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
        doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
        // The range counts the characters the shape shows: its own, or its master's when it holds none of its own. The shape then holds a `Text` of its own, made of the master's parts with the
        // change in them, so the markers, the fields and the runs of the master's text are the shape's own now, as they were shown.
        match replaced(shown_parts(doc, &at), start, end, text)? {
            Some(parts) => parts,
            // Nothing is asked: nothing changes.
            None => return Ok(false),
        }
    };
    let was = doc.clone();
    // As `set_text`: the text's ODF data goes with it, for the writer to match the file's paragraphs to the new ones by content, and a text with nothing left in it is an empty `Text` of its own
    // when the shape has a master's text to override ([`kept_text`]).
    let ext = doc.shape(&at).and_then(|shape| shape.text.as_ref()).map(|t| t.ext.clone()).unwrap_or_default();
    let kept = kept_text(doc, &at, parts, ext);
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    shape.text = kept;
    settle_text(doc, page, &at)?;
    route_recalculated(doc, &was, page)?;
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

/// The parts that `parts` become when the characters `start..end` are replaced by `text`; `None` when that changes nothing. (The window's text box reads the
/// paragraphs of the text it will send through it, `formatting::editor_paragraphs`.)
pub(crate) fn replaced(parts: &[TextPart], start: usize, end: usize, text: &str) -> Result<Option<Vec<TextPart>>, Problem> {
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
