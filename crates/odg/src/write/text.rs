//! A shape's text (retext; plan Review Focus 5):
//! **only the paragraphs the edit changed are written**, and what a paragraph is made of besides its characters stays where the writer can keep it.
//!
//! * **What is compared** ([`text_edit`], on [`runs`](super::runs)): the model's text and the file's, as paragraphs, each with its characters, the cells of
//!   its `Paragraph` row and the cells of the `Character` row of each run. Two paragraphs are the same when those are, and the `IX` of a row is no part of it.
//!   A plan sets the file's paragraphs beside the model's: those that are the same are kept, those that differ are edited, and a paragraph the model has more
//!   or fewer of is inserted or taken out.
//! * **An edited paragraph** ([`Editor::retext`]). Where its runs and its row are the same and the edit lies inside one run of raw text, only that text is
//!   replaced: a paragraph with spans or other zero-width elements (a bookmark, a soft page break) keeps all of them. Any other edit writes the
//!   paragraph's content again from the model: a run with a row is a `text:span` whose automatic text style holds the row's attributes, a run with none
//!   is raw text, **and a paragraph's own style is changed only when its row is** (an automatic paragraph style holds the row's paragraph attributes). A
//!   style the part already holds with the same attributes is used; otherwise a new one is made by the naming rules. A paragraph that holds an
//!   element the writer does not map (a link, a change mark, an index mark, any other element) is refused in words.
//! * **Zero-width elements and fields** ([`zero`]): a bookmark, a reference mark (a point or the two ends of a range), a soft page break and a
//!   field the model holds as plain text are put back where R10 says in a paragraph written whole, and **each paragraph the retext writes, puts an element in or
//!   edits in place is read again to hold exactly those**. A soft page break between paragraphs is an anchor at the start of the paragraph after it, and is carried over like
//!   any element when that paragraph is written whole or taken out ([`Editor::break_zeros`]; it is written between paragraphs where it lands on a paragraph's start,
//!   [`Editor::take_between`]). The retext reads in two passes: the first reads each paragraph the plan edits (not in place) or takes
//!   out, for the elements it holds and where each stood ([`Editor::plan_paragraphs`]), and carries them over, block by block ([`Editor::land_all`]: the
//!   paragraphs the plan keeps, and those it edits in place, keep theirs and part the blocks), by the block's common prefix and suffix; the second writes. A
//!   paragraph with elements has its content written in one pass ([`Editor::content_with_zeros`]): an element goes at the top level at the paragraph's start or
//!   end or between two runs, inside the span of a run that holds it, and a field inside the span of the run it shows its characters in; no `text:span` of the
//!   file is kept as its bytes in it. A field the model holds as a field (a `fld`: a page number, a page count, a file name, a date or a time
//!   that is not fixed) whose characters a refresh changed is written again, its element's tags around the new characters ([`zero`]'s pairing); any other
//!   kept field whose characters changed is written as the new characters, and the save says so
//!   ([`crate::SaveNote::FieldWritten`]).
//! * **An inserted paragraph** is a `text:p` put after the paragraph of the file before it (before the one after it when none is), **a list item when it
//!   follows or precedes one whose row is its own** (a new item of that list: `text:list-item` holds the paragraph, and the style is the neighbour's). **Where no
//!   neighbour has its row** it is put outside any list (after the list the paragraph before it is in, or before the one after it is in: the reader skips a
//!   `text:p` that is a child of a `text:list`), and between two items of one list it is refused in words; one with a list's row there is the item of a
//!   `text:list` this makes, with an automatic list style, the paragraphs of a row at one place being the items of one list. **The paragraphs
//!   put in at one place are written in the model's order**: one that would fall before a paragraph already written outside a list is written after it, outside
//!   the list too, and one that would fall between two items of one list is refused ([`Editor::retext`]). A paragraph taken out takes its list item with it when
//!   it is the item's only child.
//! * **White space**: a first space is written as itself, each further space of a row as `text:s` with `text:c` (at most the reader's bound,
//!   [`MOST_SPACES`], in one element: a longer run is written as several), a leading or trailing space, U+0009 as `text:tab` and U+2028 as
//!   `text:line-break`. In a paragraph with no element a run is written alone, so a space at its edge is `text:s`; **in one with elements the characters are
//!   written in one pass** and a space is `text:s` where a plain one would not read back (R10 item 6: after another space with only elements between, at the
//!   start of the paragraph after elements alone, after a field, and at the end).
//! * **What a count stands for is never built** (security finding): a `text:s` is a count, clamped to 1 to [`MOST_SPACES`] as the reader clamps it,
//!   and the characters a paragraph reads as are made only up to the length the model says the paragraph has: more than that is a file that is not the
//!   one the model came from, refused before the characters are built.
//! * **The check** ([`check_paragraphs`]): each paragraph written is read again from the bytes made, by the same tokens, and must be the text the model has,
//!   with the zero-width elements put in it each at its offset and with its bytes;
//!   a paragraph put in must have the paragraph style the writer named for it, in the place its number gives it (so two paragraphs of one text put in the wrong
//!   order are told apart by their styles), and a `text:p` that is the child of a list is refused (the reader skips it). The styles made are read back by
//!   [`super::check_styles`], and the package made is read again through the reader when a paragraph was put in, taken out or made a list's.
//! * **Cost**: every paragraph and run of each model is walked once and charged; the plan is linear; every attribute and element written is charged before it
//!   is made. A text of more than [`MOST_EDITED_TEXT`] bytes or [`super::runs::MOST_PARAGRAPHS`] paragraphs is not rewritten.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use diacraft_container::xml_scan::{AttrSpan, tag_attributes};
use diacraft_doc::{Shape, TextPart};

use super::attrs::TagEdit;
use super::emit::Pfx;
use super::frame::Editor;
use super::lengths::length_text;
use super::runs::{self, Props, Rows, Step, View};
use super::scan::{Ask, Fam, Kid, PartKind, StyleDef, scan_part_asking};
use super::stylemap::{Op, Val};
use super::styles::Written;
use super::zero::{self, Landed, Target, Zero};
use super::{SaveNote, WriteError, WriteWork};
use crate::limits::Limits;
use crate::read::names::Ns;
use crate::read::readback::MOST_SPACES;

pub(crate) mod model;

/// The most bytes of text a retext holds beside the model's own (the paragraphs of the old text and of the new).
pub(crate) const MOST_EDITED_TEXT: usize = 16 << 20;

/// The words of a refusal of a list whose bullet is not one character.
const WORDS_BULLET_STR: &str = "a bullet that is more than one character (a list level holds one character, `text:bullet-char`; R25)";

/// The words of a refusal of a text format that no character carries.
const WORDS_UNUSED_ROW: &str = "a text format set on a shape that has no character to carry it (the cells of a run's or a paragraph's row that no paragraph of the text uses are not written, and the file would read as having none)";

/// The bytes of memory a retext holds for each paragraph of a text (its view: the characters' place, the row, the runs; and the place of its element), and for
/// each paragraph of the plan (a step, the place the next paragraph is looked for from, and the list of the paragraphs of the file): about 240 in all, measured by
/// the review (479 MB at the cap of 2,000,000 paragraphs), counted a little over.
const VIEW_BYTES: u64 = 160;
const PLAN_BYTES: u64 = 96;

/// The bytes of memory counted for each span of the file a retext keeps as its bytes until the paragraph is written (the span's place, its bytes and its style name).
const KEPT_BYTES: u64 = 160;

/// What the style of one run's span costs to hold: the slot of the list ([`NearCache::run_styles`]) and the name's header, beside the bytes of the name.
const RUN_STYLE_BYTES: u64 = 32;

/// The most places the table of the styles the runs of a paragraph need is made with room for at once: a hint, so that a paragraph of millions of runs does not ask for
/// the room of all of them before one is looked at (the table grows as the runs are walked, and each step of the walk is charged).
const MOST_NEEDS_RESERVED: usize = 65_536;

/// The bytes of memory counted for the allocation the characters of one text or field token are in, besides the characters themselves (the allocator's own header
/// and the rounding up).
const ALLOCATION_BYTES: u64 = 16;

/// The bytes of memory a paragraph's tokens hold while it is planned: a token for each piece of the content, and for each text or
/// field the characters it shows as a list of `char`, four bytes each for the room asked for (`text_chars` reserves a character for each byte of the raw text).
fn tokens_bytes(tokens: &[Token]) -> u64 {
    tokens.iter().fold(0u64, |sum, token| {
        let characters = match token {
            Token::Text { chars, .. } | Token::Field { chars, .. } => (chars.capacity() as u64).saturating_mul(4).saturating_add(ALLOCATION_BYTES),
            _ => 0,
        };
        sum.saturating_add(size_of::<Token>() as u64).saturating_add(characters)
    })
}

/// The next `n` characters of the paragraph's `text`, from where `chars` (the walk of its characters and their places) stands: a range that is not in the text is a
/// fault, and not an empty piece (the characters would be lost).
fn take<'t>(text: &'t str, chars: &mut std::iter::Peekable<std::str::CharIndices<'t>>, n: usize) -> Result<&'t str, WriteError> {
    let start = chars.peek().map_or(text.len(), |(b, _)| *b);
    for _ in 0..n {
        chars.next();
    }
    let end = chars.peek().map_or(text.len(), |(b, _)| *b);
    text.get(start..end).ok_or(WriteError::Mismatch("characters of a paragraph that are not in its text"))
}

/// The words of a fault of the scan: a paragraph it found that is not in the part.
const WORDS_NOT_THERE: &str = "a paragraph the scan found that is not in the part";

/// The words of a fault of the part read: a tag of a paragraph that is not text.
const WORDS_NOT_TEXT: &str = "a tag of a paragraph that is not text";

/// The words the refusal of a text whose lists would pass the memory a save may hold is told by; a retext is refused as a save that holds too much memory.
pub(crate) const MEMORY_WORDS: &str = "memory for the paragraphs of a retext";

/// What a text edit changes: the paragraphs the file holds as the baseline model has them (`old`), the paragraphs of the model (`new`), and the plan that
/// makes the one the other. The characters of a paragraph that is kept are let go.
#[derive(Clone, Debug)]
pub(crate) struct TextEdit {
    pub old: Vec<View>,
    pub new: Vec<View>,
    pub steps: Vec<Step>,
}

impl TextEdit {
    /// The paragraphs the save writes, as the model has them: the place each has in the text after the save and what it reads as.
    pub(crate) fn written(&self) -> Vec<(usize, String)> {
        self.steps
            .iter()
            .filter_map(|s| match *s {
                Step::Edit { new, .. } | Step::Insert { new } => self.new.get(new).map(|v| (new, v.chars.clone())),
                _ => None,
            })
            .collect()
    }

    /// Whether any paragraph the save writes holds a character: a custom shape that holds text has its frame held (`draw:auto-grow-height`).
    pub(crate) fn holds_text(&self) -> bool {
        self.steps.iter().any(|s| match *s {
            Step::Edit { new, .. } | Step::Insert { new } => self.new.get(new).is_some_and(|v| !v.chars.is_empty()),
            _ => false,
        })
    }
}

/// The change from the text of `old` to the text of `new`: `None` when the paragraphs are the same (the rows of a text that nothing in it uses are no
/// change a file shows), an edit, or the words of why not.
pub(crate) fn text_edit(
    old: &Shape,
    new: &Shape,
    palette: &dyn Fn(i64) -> Option<String>,
    steps: &mut u64,
    room: &mut u64,
) -> Result<Option<TextEdit>, &'static str> {
    // The lists of the paragraphs of the two texts, and the plan, are charged to the memory a save may hold before they are made: the paragraphs are counted
    // from the line feeds of the texts (a step for each part), and what they come to is taken from `room`, what is left of the estimate the gate made.
    let paragraphs = |s: &Shape| -> usize {
        let parts = s.text.iter().flat_map(|t| t.parts.iter());
        parts
            .map(|p| match p {
                TextPart::Chars(c) => c.bytes().filter(|b| *b == b'\n').count(),
                _ => 0,
            })
            .fold(1usize, usize::saturating_add)
    };
    *steps = steps
        .saturating_add(old.text.as_ref().map_or(0, |t| t.parts.len()) as u64)
        .saturating_add(new.text.as_ref().map_or(0, |t| t.parts.len()) as u64);
    let (before, after) = (paragraphs(old), paragraphs(new));
    let need = (before.saturating_add(after) as u64).saturating_mul(VIEW_BYTES).saturating_add((before.max(after) as u64).saturating_mul(PLAN_BYTES));
    *room = room.checked_sub(need).ok_or(MEMORY_WORDS)?;
    let mut a = {
        let (mut characters, mut paragraphs) = (Rows::of(&old.sheet, false, palette, steps), Rows::of(&old.sheet, true, palette, steps));
        runs::views(old.text.as_ref(), &mut characters, &mut paragraphs, steps)?
    };
    let mut b = {
        let (mut characters, mut paragraphs) = (Rows::of(&new.sheet, false, palette, steps), Rows::of(&new.sheet, true, palette, steps));
        runs::views(new.text.as_ref(), &mut characters, &mut paragraphs, steps)?
    };
    let plan = runs::plan(&a, &b, steps);
    // The rows of a text that no character of it uses are not written, and the file would read as having none. With nothing else changed, a row the new
    // sheet has and the old had no like of is a format with no character to carry it (the review; a renumbering of the rows is no such
    // row). **Beside another edit** such a row is lost the same way, and it is refused when it is a format a person sees: the `Character` row in force at a
    // paragraph with no character (a format on an empty paragraph, which sets the height of its line, parked 7, probe
    // EMPTY_PARA_BOLD). A row that no marker of the text holds in force anywhere, or that only a stretch of no character inside a paragraph that has some does,
    // is no format a person sees (the engine leaves one after the characters of a formatted run are deleted), and is not refused.
    let all_kept = plan.iter().all(|s| matches!(s, Step::Keep { .. }));
    let was = runs::row_keys(&old.sheet, steps);
    let mut now = runs::row_keys(&new.sheet, steps);
    now.retain(|k| !was.contains(k));
    if !now.is_empty() {
        if all_kept {
            return Err(WORDS_UNUSED_ROW);
        }
        // A row a character of the new text has is used; what is left holds no character.
        for v in &b {
            // One for the paragraph, and one for each of its runs.
            *steps = steps.saturating_add(1).saturating_add(v.runs.len() as u64);
            if let Some(p) = &v.pp {
                now.remove(&(true, p.key.clone()));
            }
            for r in &v.runs {
                if let Some(p) = &r.props {
                    now.remove(&(false, p.key.clone()));
                }
            }
        }
        if b.iter().any(|v| v.empty.as_ref().is_some_and(|p| now.contains(&(false, p.key.clone())))) {
            return Err(WORDS_UNUSED_ROW);
        }
    }
    if all_kept {
        return Ok(None);
    }
    // What is kept is not written, and its characters are not needed again.
    for s in &plan {
        if let Step::Keep { old, new } = *s {
            if let Some(v) = a.get_mut(old) {
                v.chars = String::new();
            }
            if let Some(v) = b.get_mut(new) {
                v.chars = String::new();
            }
        }
    }
    Ok(Some(TextEdit { old: a, new: b, steps: plan }))
}

/// What a white space element of a paragraph is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    /// `text:s` of this many spaces, clamped as the reader clamps it.
    Spaces(usize),
    Tab,
    LineBreak,
}

impl Space {
    /// How many characters it stands for.
    fn len(self) -> usize {
        match self {
            Space::Spaces(n) => n,
            _ => 1,
        }
    }

    fn push_to(self, out: &mut String) {
        match self {
            Space::Spaces(n) => out.extend(std::iter::repeat_n(' ', n)),
            Space::Tab => out.push('\t'),
            Space::LineBreak => out.push('\u{2028}'),
        }
    }
}

/// A token of a paragraph's content.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    /// Raw text from here to here (bytes), the characters it reads as, and whether it holds a reference (then its bytes are not its characters).
    Text { span: (usize, usize), chars: Vec<char>, reference: bool },
    /// `text:s`, `text:tab` or `text:line-break`: one element each, standing for characters that are not built until they are asked for.
    Element(Space),
    /// A `text:span` start tag (where it starts, in the part), and the style it names: no characters.
    SpanOpen { style: Option<String>, at: usize },
    /// A `text:span` end tag (where it ends, in the part).
    SpanClose { end: usize },
    /// A zero-width element (a bookmark, a reference mark, a soft page break): no characters. Where its element is, from its `<` to its end.
    Mark { bytes: (usize, usize) },
    /// A text field the model holds as plain text: where its element is, the characters it shows (they read as text, white space collapsed as the reader
    /// collapses it), and its local name.
    Field { bytes: (usize, usize), chars: Vec<char>, kind: &'static str },
}

/// Where the tag that starts at `at` ends (after its `>`): a `>` inside a quoted value does not end it. `None` for a tag that is not closed.
fn tag_end(content: &[u8], at: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in content.get(at..)?.iter().enumerate() {
        match (quote, *b) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(*b),
            (None, b'>') => return Some(at + i + 1),
            (None, _) => {}
        }
    }
    None
}

/// Where the end tag `</name>` that stands at `at` ends, when one does.
fn end_tag_after(content: &[u8], at: usize, name: &str) -> Option<usize> {
    let tail = content.get(at..)?;
    let want = name.len() + 3;
    let ok = tail.get(..2) == Some(b"</") && tail.get(2..2 + name.len()) == Some(name.as_bytes()) && tail.get(2 + name.len()) == Some(&b'>');
    ok.then_some(at + want)
}

/// Whether an element of a paragraph is a drawing object the reader holds whole where it stands (`read/text.rs`, a frame anchored as a character, a shape, a group): an element
/// of the drawing or the 3-D vocabulary, by the prefixes the model writer writes them under.
fn drawing_object(name: &str) -> bool {
    name.split_once(':').is_some_and(|(prefix, local)| matches!(prefix, "draw" | "dr3d") && !local.is_empty())
}

/// Where the element `name` whose start tag ends at `at` ends: after the end tag that closes it, elements of the same name inside it counted (a frame in a text box in a frame). Comments, character
/// data sections and processing instructions are stepped over. Each step moves on, so the walk is linear in the bytes of the element; `None` for an element that is not closed.
fn element_end(content: &[u8], at: usize, name: &str) -> Option<usize> {
    let mut depth = 1usize;
    let mut here = at;
    loop {
        let lt = here.checked_add(content.get(here..)?.iter().position(|b| *b == b'<')?)?;
        let tail = content.get(lt..)?;
        let skip_to = |end: &[u8]| -> Option<usize> {
            let found = tail.windows(end.len()).position(|w| w == end)?;
            lt.checked_add(found)?.checked_add(end.len())
        };
        if tail.starts_with(b"<!--") {
            here = skip_to(b"-->")?;
            continue;
        }
        if tail.starts_with(b"<![CDATA[") {
            here = skip_to(b"]]>")?;
            continue;
        }
        if tail.starts_with(b"<?") {
            here = skip_to(b"?>")?;
            continue;
        }
        let close = tag_end(content, lt)?;
        let tag = std::str::from_utf8(content.get(lt + 1..close - 1)?).ok()?;
        let closing = tag.starts_with('/');
        let tag_name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or_default();
        if tag_name == name {
            if closing {
                depth -= 1;
                if depth == 0 {
                    return Some(close);
                }
            } else if !tag.ends_with('/') {
                depth = depth.checked_add(1)?;
            }
        }
        here = close;
    }
}

/// The characters of raw text as a list, references resolved, the room for them asked for first.
fn text_chars(raw: &[u8]) -> Option<(Vec<char>, bool)> {
    let text = std::str::from_utf8(raw).ok()?;
    let reference = text.contains('&');
    let resolved;
    let source: &str = if reference {
        resolved = crate::package::unescape(raw);
        &resolved
    } else {
        text
    };
    // The characters of a run of raw text are held four bytes each: the room is asked for, and a paragraph that has not the room is not mapped.
    let mut chars: Vec<char> = Vec::new();
    chars.try_reserve_exact(source.len()).ok()?;
    chars.extend(source.chars());
    Some((chars, reference))
}

/// The tokens of a paragraph's content, and whether it holds a span, a zero-width element or a field. `None` for content the writer does not map (any other
/// element, a comment, a zero-width element or a field that is not written whole with its end tag at once). `text_ns` says what the element named is, by the
/// local name it has in the text namespace (`""` for any other).
fn tokenize(bytes: &[u8], from: usize, to: usize, text_ns: &dyn Fn(&str) -> Option<&'static str>) -> Option<(Vec<Token>, bool)> {
    let content = bytes.get(from..to)?;
    let mut tokens = Vec::new();
    let mut structure = false;
    let mut at = 0usize;
    while at < content.len() {
        if content.get(at) == Some(&b'<') {
            let close = tag_end(content, at)?;
            let tag = std::str::from_utf8(content.get(at + 1..close - 1)?).ok()?;
            let closing = tag.starts_with('/');
            let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or_default();
            let kind = text_ns(name)?;
            let empty = tag.ends_with('/');
            let mut next = close;
            match (kind, closing) {
                ("span", closing) => {
                    structure = true;
                    if closing {
                        tokens.push(Token::SpanClose { end: from + close });
                    } else {
                        let prefix = name.split_once(':').map_or("", |(p, _)| p);
                        let wanted = format!("{prefix}:style-name");
                        let mut spans: Vec<AttrSpan> = Vec::new();
                        let style = tag_attributes(content, at, close, &mut spans).and_then(|_| {
                            let found = spans.iter().find(|a| content.get(a.name.0..a.name.1) == Some(wanted.as_bytes()))?;
                            Some(crate::package::unescape(content.get(found.value.0..found.value.1)?))
                        });
                        tokens.push(Token::SpanOpen { style, at: from + at });
                        if empty {
                            tokens.push(Token::SpanClose { end: from + close });
                        }
                    }
                }
                (kind, false) if zero::mark_name(kind).is_some() => {
                    // No content: written empty, or as the start tag with its end tag at once.
                    let end = if empty { close } else { end_tag_after(content, close, name)? };
                    structure = true;
                    tokens.push(Token::Mark { bytes: (from + at, from + end) });
                    next = end;
                }
                (kind, false) if zero::field_name(kind).is_some() => {
                    // The characters it shows, and no element inside it.
                    let (chars, end) = if empty {
                        (Vec::new(), close)
                    } else {
                        let lt = content.get(close..)?.iter().position(|b| *b == b'<')? + close;
                        let end = end_tag_after(content, lt, name)?;
                        (text_chars(content.get(close..lt)?)?.0, end)
                    };
                    structure = true;
                    tokens.push(Token::Field { bytes: (from + at, from + end), chars, kind });
                    next = end;
                }
                ("tab", false) if empty => tokens.push(Token::Element(Space::Tab)),
                ("line-break", false) if empty => tokens.push(Token::Element(Space::LineBreak)),
                // A drawing object: zero-width, as a mark is, and whole, its own content and all.
                ("", false) if drawing_object(name) => {
                    let end = if empty { close } else { element_end(content, close, name)? };
                    structure = true;
                    tokens.push(Token::Mark { bytes: (from + at, from + end) });
                    next = end;
                }
                ("s", false) if empty => {
                    // The count is clamped to 1 to the reader's bound, as the reader clamps it; no string is made of it here.
                    // The attribute is found by the parser of start tags, in either quote and with the references of its value resolved.
                    let prefix = name.split_once(':').map_or("", |(p, _)| p);
                    let wanted = format!("{prefix}:c");
                    let mut spans: Vec<AttrSpan> = Vec::new();
                    let count = tag_attributes(content, at, close, &mut spans)
                        .and_then(|_| {
                            let found = spans.iter().find(|a| content.get(a.name.0..a.name.1) == Some(wanted.as_bytes()))?;
                            let value = crate::package::unescape(content.get(found.value.0..found.value.1)?);
                            crate::read::units::integer(&value)
                        })
                        .unwrap_or(1);
                    tokens.push(Token::Element(Space::Spaces(usize::try_from(count.clamp(1, MOST_SPACES)).unwrap_or(1))));
                }
                _ => return None,
            }
            at = next;
        } else {
            let end = content.get(at..)?.iter().position(|b| *b == b'<').map_or(content.len(), |p| at + p);
            let (chars, reference) = text_chars(content.get(at..end)?)?;
            tokens.push(Token::Text { span: (from + at, from + end), chars, reference });
            at = end;
        }
    }
    Some((tokens, structure))
}

/// How many characters a run of raw text reads as: each run of white space is one, as the reader collapses it (indentation and the line feeds that
/// lay a paragraph out in the file read as nothing or as one space, however many characters they are).
fn collapsed(chars: &[char]) -> usize {
    let (mut n, mut after_space) = (0usize, false);
    for c in chars {
        let space = matches!(c, ' ' | '\t' | '\n' | '\r');
        if !(space && after_space) {
            n += 1;
        }
        after_space = space;
    }
    n
}

/// How many characters the tokens stand for, white space of raw text collapsed as the reader collapses it and a counted space at its count:
/// counted, nothing built. What stands for much more than the model says is what a count of spaces can make (a few bytes of file for 65,536
/// characters), and not what raw text is, which is as long as the file it is in.
fn stands_for(tokens: &[Token]) -> usize {
    tokens
        .iter()
        .map(|t| match t {
            Token::Text { chars, .. } | Token::Field { chars, .. } => collapsed(chars),
            Token::Element(s) => s.len(),
            Token::SpanOpen { .. } | Token::SpanClose { .. } | Token::Mark { .. } => 0,
        })
        .fold(0usize, usize::saturating_add)
}

/// Whether the tokens stand for so many more characters than `budget` (the model's paragraph) that they are not built: a count of spaces is a few bytes of file
/// for up to 65,536 characters, and the paragraph the model holds is the most any file can be.
fn stands_for_too_much(tokens: &[Token], budget: usize) -> bool {
    stands_for(tokens) > budget.saturating_add(budget / 2).saturating_add(8)
}

/// The reader's white space state between two characters: whether white space here adds nothing (the start, or a collapsed space was just written), and
/// whether the last character written is a collapsed space (which the paragraph's end takes off).
type State = (bool, bool);

/// The state at the start of a paragraph.
const START: State = (true, false);

/// How many characters `chars` read as, the state they are read in moved on to what follows them: white space after white space adds nothing.
fn walk(chars: &[char], state: &mut State) -> usize {
    let mut n = 0usize;
    for c in chars {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !state.0 {
                n += 1;
                state.1 = true;
            }
            state.0 = true;
        } else {
            n += 1;
            *state = (false, false);
        }
    }
    n
}

/// What the reader makes of a paragraph's tokens: its characters (when asked for), how many they are, and for each zero-width element and field (by the
/// place of its token) the offset of the character it stands before and the characters it shows.
struct Reading {
    chars: String,
    total: usize,
    placed: Vec<(usize, usize, usize)>,
}

/// The characters a paragraph's tokens read as, white space collapsed as the reader collapses it: a run of spaces, tabs and line feeds of raw text (and of a
/// field's characters) is one space, one at the start of the paragraph goes, and **the collapsed space that is the paragraph's last character goes** (a space
/// that `text:s` stands for does not). **Nothing past `budget` characters is built**: a paragraph that stands for more (counted first) is `None`, and the
/// reader's own cap on a paragraph's text is the most any file can be. With `build` false only the count and the places are made.
fn reading(tokens: &[Token], budget: usize, build: bool) -> Option<Reading> {
    if stands_for_too_much(tokens, budget) {
        return None;
    }
    let mut out = String::new();
    if build {
        out.try_reserve(budget).ok()?;
    }
    let mut placed: Vec<(usize, usize, usize)> = Vec::new();
    let mut count = 0usize;
    // The reader's state: a collapsed space was just written, or nothing was (the start), so that more white space adds nothing; and whether the last character
    // is a collapsed space.
    let (mut previous_space, mut trailing) = (true, false);
    // The characters of raw text (or of a field), collapsed as the reader collapses them.
    fn take(build: bool, chars: &[char], out: &mut String, count: &mut usize, previous_space: &mut bool, trailing: &mut bool) {
        for c in chars {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                if !*previous_space {
                    if build {
                        out.push(' ');
                    }
                    *count += 1;
                    *trailing = true;
                }
                *previous_space = true;
            } else {
                if build {
                    out.push(*c);
                }
                *count += 1;
                *previous_space = false;
                *trailing = false;
            }
        }
    }
    for (at, t) in tokens.iter().enumerate() {
        match t {
            Token::Text { chars, .. } => take(build, chars, &mut out, &mut count, &mut previous_space, &mut trailing),
            Token::Field { chars, .. } => {
                let start = count;
                take(build, chars, &mut out, &mut count, &mut previous_space, &mut trailing);
                placed.try_reserve(1).ok()?;
                placed.push((at, start, count - start));
            }
            Token::Element(space) => {
                if build {
                    space.push_to(&mut out);
                }
                count = count.saturating_add(space.len());
                previous_space = false;
                trailing = false;
            }
            Token::Mark { .. } => {
                placed.try_reserve(1).ok()?;
                placed.push((at, count, 0));
            }
            Token::SpanOpen { .. } | Token::SpanClose { .. } => {}
        }
    }
    if trailing {
        if build {
            out.pop();
        }
        count -= 1;
    }
    // An element after the collapsed space that goes is at the paragraph's end.
    for p in &mut placed {
        p.1 = p.1.min(count);
        p.2 = p.2.min(count - p.1);
    }
    (count <= budget).then_some(Reading { chars: out, total: count, placed })
}

/// The characters of a paragraph's tokens: [`reading`], built.
fn read_chars(tokens: &[Token], budget: usize) -> Option<String> {
    reading(tokens, budget, true).map(|r| r.chars)
}

/// A field's element, `element` from its `<` to the end of its end tag, written again with `shown` as its characters: the start tag and the end tag as they are,
/// and the content between them replaced by `shown`, escaped. `None` for an element with no content to replace (an empty one, or bytes that are
/// not a start tag, content and an end tag). The start tag ends at the first `>` outside a quoted attribute value.
pub(crate) fn refreshed_field(element: &str, shown: &str) -> Option<String> {
    let mut quote: Option<u8> = None;
    let mut start_end = None;
    for (i, b) in element.bytes().enumerate().skip(1) {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'>') => {
                start_end = Some(i + 1);
                break;
            }
            _ => {}
        }
    }
    let start_end = start_end?;
    let head = element.get(..start_end)?;
    if head.ends_with("/>") {
        return None;
    }
    let end_start = element.rfind("</").filter(|at| *at >= start_end)?;
    let tail = element.get(end_start..)?;
    Some(format!("{head}{}{tail}", escape_text(shown)))
}

pub(crate) fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The content that reads as `chars`, as R24 writes it: `text:s`, `text:tab` and `text:line-break` under the prefix `p`, `declare` on each. A run of
/// counted spaces longer than the reader's bound is written as several elements.
pub(crate) fn encode(chars: &str, p: &str, declare: &str) -> String {
    let mut out = String::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut String| {
        out.push_str(&escape_text(plain));
        plain.clear();
    };
    let total = chars.chars().count();
    let mut it = chars.chars().peekable();
    let mut i = 0usize;
    while let Some(c) = it.next() {
        i += 1;
        match c {
            ' ' => {
                let mut n = 1usize;
                while it.peek() == Some(&' ') {
                    it.next();
                    n += 1;
                }
                let edge = i == 1 || i + n - 1 == total;
                i += n - 1;
                // A first space is written as itself, unless it is at an edge; every further space of the row is counted.
                let (literal, mut counted) = if edge { (0, n) } else { (1, n - 1) };
                if literal == 1 {
                    plain.push(' ');
                }
                if counted > 0 {
                    flush(&mut plain, &mut out);
                }
                while counted > 0 {
                    let now = counted.min(usize::try_from(MOST_SPACES).unwrap_or(65_536));
                    counted -= now;
                    if now == 1 {
                        out.push_str(&format!("<{p}:s{declare}/>"));
                    } else {
                        out.push_str(&format!("<{p}:s{declare} {p}:c=\"{now}\"/>"));
                    }
                }
            }
            '\t' => {
                flush(&mut plain, &mut out);
                out.push_str(&format!("<{p}:tab{declare}/>"));
            }
            '\u{2028}' => {
                flush(&mut plain, &mut out);
                out.push_str(&format!("<{p}:line-break{declare}/>"));
            }
            other => plain.push(other),
        }
    }
    flush(&mut plain, &mut out);
    out
}

/// A paragraph element of the file: where it and its list item (when it is a list's) are.
#[derive(Clone, Copy, Debug)]
struct ParaAt {
    open: (usize, usize),
    empty: bool,
    close_start: usize,
    end: usize,
    scope: u32,
    lead: usize,
    item: Option<ItemAt>,
    /// The outermost `text:list` the paragraph is in, as the bytes it takes: where a paragraph that is not the list's goes beside it.
    list: Option<(usize, usize)>,
    /// Where the first soft page break that stands right before the paragraph, between it and the paragraph before it, begins (R10 item 1: the break is an anchor at
    /// the start of this paragraph, which stays with it while the plan keeps it): a paragraph put in before this one goes before the break.
    brk: Option<usize>,
    /// The soft page breaks that stand right before the paragraph, as a range of [`NearCache::breaks`]: the anchors a block that writes it whole or takes it out carries
    /// over (R10 item 3).
    brks: (usize, usize),
}

/// The list item a paragraph is the child of.
#[derive(Clone, Copy, Debug)]
struct ItemAt {
    lead: usize,
    end: usize,
    /// The paragraph is the item's only child: the item goes and comes with it.
    alone: bool,
    header: bool,
}

impl ParaAt {
    /// What a paragraph that is not a list's is put beside: the outermost list this one is in, else this paragraph.
    fn top(&self) -> (usize, usize) {
        self.list.unwrap_or((self.lead, self.end))
    }

    /// Where a paragraph goes that is put before this one's unit at the container's level: the unit's start, or the soft page break before it when it has one.
    fn before(&self, unit_lead: usize) -> usize {
        self.brk.map_or(unit_lead, |brk| brk.min(unit_lead))
    }

    /// The unit that goes and comes with the paragraph: its list item when it is the item's only child, else the paragraph. `None` for a paragraph in an item that
    /// holds more (or a list header), which a save does not insert beside or take out.
    fn unit(&self) -> Option<(usize, usize)> {
        match self.item {
            None => Some((self.lead, self.end)),
            Some(item) if item.alone && !item.header => Some((item.lead, item.end)),
            Some(_) => None,
        }
    }
}

/// A list the save makes, held until the paragraphs that go in it are all given: the paragraphs with one list's row that come in a row at one place are the
/// items of one `text:list`.
struct PendingList {
    place: usize,
    props: Arc<Props>,
    /// The prefix of the text namespace at the place, and the declarations the list's start tag carries for the prefixes the place does not bind.
    text: String,
    declare: String,
    items: Vec<String>,
}

/// What the retext of one shape found of the paragraphs of the file that the model's paragraphs are put beside: the styles each one's spans name for its runs,
/// and the style its start tag names, each by the paragraph's place, asked for once.
#[derive(Default)]
struct NearCache {
    /// For each paragraph of the file: whether a paragraph the plan puts in has it for a neighbour (the one before or the one after it, by the file's paragraphs): such a
    /// paragraph takes the neighbour's style and the styles its spans name for the runs of the same row, **as the file has them**, so the neighbour's styles are not
    /// edited where they stand ([`Editor::in_place_styles`]).
    near_inserted: Vec<bool>,
    reuse: HashMap<usize, Rc<HashMap<String, String>>>,
    /// For each paragraph of the file that is written whole, the style the span of each run of its baseline names (`None` for a run that lies in no one span that
    /// names a style): where the style of a run whose row changed is made from.
    run_styles: HashMap<usize, Rc<Vec<Option<String>>>>,
    names: HashMap<usize, Option<String>>,
    /// Where the last paragraph put in at this place was put, and whether that is outside any list: the paragraphs put in at one place are written in the
    /// model's order, so none goes before it (forgotten at the next paragraph the file keeps, edits or loses).
    last: Option<(usize, bool)>,
    /// The paragraphs put in, by their number in the model's text, with the paragraph style written for each: what the check of the bytes made looks for.
    inserted: super::StyleNames,
    /// What the check of the bytes made expects of the zero-width elements of the paragraphs written, when the text holds any (`checking`).
    expect: Vec<zero::Expect>,
    checking: bool,
    /// Every soft page break between paragraphs of the shape, as where it begins (with the white space before it) and ends, in order: [`ParaAt::brks`] ranges over it.
    breaks: Vec<(usize, usize)>,
}

/// How the paragraph a text edit changes is written (the first pass of [`Editor::retext`]).
enum EditPlan {
    /// The edit lies inside one run of raw text, the runs and the row the same: the bytes it replaces and what it puts there. Every element of the paragraph
    /// stays where it is (R10 item 2).
    InPlace { from: usize, to: usize, text: String },
    /// The paragraph's content is written again from the model. `kept` are the spans of the file that are unchanged runs, by the run's place in the new
    /// paragraph (a span that holds an element is none of them).
    Whole { kept: HashMap<usize, KeptSpan> },
}

/// What the first pass of a retext found: how each step is written, and the elements of the paragraphs it edits (not in place) or takes out, by the paragraph's
/// number in the file.
struct Planned {
    plans: Vec<Option<EditPlan>>,
    found: HashMap<usize, Vec<Zero>>,
}

/// What a retext of one shape gives for the check of the bytes made: the paragraph style written for each paragraph put in, and what each paragraph that
/// took an element is expected to hold of them.
pub(crate) struct Retexted {
    pub styles: super::StyleNames,
    pub expect: Vec<zero::Expect>,
}

/// The words of a refusal of a paragraph added or taken out next to a list item that holds more than the paragraph.
const WORDS_ITEM: &str = "a paragraph added beside or taken out of a list item that holds more than one paragraph, a list header or another list";

/// The words of a refusal of a paragraph that holds what the writer does not map.
const WORDS_UNMAPPED: &str = "a paragraph that holds an element or a reference the writer does not map";

/// The words of a refusal of a paragraph that holds a link (the placing does not cover one).
const WORDS_LINK: &str = "a paragraph that holds a link (a `text:a` element), which the writer does not keep in a paragraph it edits (the specification leaves open how: Q4)";

/// The words of a refusal of a format that cuts a field's characters in two runs.
const WORDS_FIELD_CUT: &str = "a text format that cuts through the characters of a field (a field shows its characters in one run, and the specification does not say how one cut in two is kept)";

/// The words of a refusal of a zero-width element put where the prefixes its bytes use are not those in force.
const WORDS_ZERO_SCOPE: &str =
    "a zero-width element put in a paragraph where the namespace prefixes its bytes use are not the ones in force (it is not copied there)";

/// The words of a refusal of a paragraph put between two items of one list when neither has its row.
const WORDS_BETWEEN: &str =
    "a paragraph put between two items of a list, where neither has its row (the specification does not say how a list is split in two)";

/// The words of a refusal of a list item whose paragraph row changed.
const WORDS_LIST_CHANGED: &str = "a paragraph of a list whose paragraph settings changed (its settings are the list's)";

/// The words of a refusal of a font family that cannot be written as the name of a declaration.
const WORDS_FACE: &str =
    "a font family that cannot be written as the declaration of a font face (it is empty or holds a character XML does not allow)";

/// The styles of one family that a part holds, by their attributes: the canonical key of a style to its place among the part's definitions. It reads as the map it holds
/// (`get`, `contains_key`, `len`), and **drops keys by one way only, [`StyleKeys::retain`], a walk of every key that counts the keys it goes over**. A save
/// pays for that walk at the end of the part ([`Editor::count_index_walks`]), so that a walk of the index for each edit in place, which is quadratic in the edits and was
/// uncharged (the review, B4), moves the steps of the save and is seen by the counted tests. Nothing walks it today: a style edited where it stands is a miss
/// when the index is asked ([`Editor::existing_style`]).
#[derive(Default)]
pub(crate) struct StyleKeys {
    keys: HashMap<String, usize>,
    /// The keys the walks of [`StyleKeys::retain`] have gone over, in all.
    walked: u64,
}

impl std::ops::Deref for StyleKeys {
    type Target = HashMap<String, usize>;

    fn deref(&self) -> &HashMap<String, usize> {
        &self.keys
    }
}

impl StyleKeys {
    fn new(keys: HashMap<String, usize>) -> StyleKeys {
        StyleKeys { keys, walked: 0 }
    }

    /// Keeps the keys `keep` says yes to, and counts every key it asks about: the cost of the walk, whatever it keeps.
    #[cfg_attr(not(test), expect(dead_code, reason = "nothing walks the index; a walk that is put back is counted here"))]
    fn retain(&mut self, mut keep: impl FnMut(&String, &mut usize) -> bool) {
        self.walked = self.walked.saturating_add(self.keys.len() as u64);
        self.keys.retain(|key, place| keep(key, place));
    }
}

/// The text styles of one part's save: what the part declares, what was made or found for each row the save has met, and what the part's own styles are by
/// their attributes (asked for once, the first time one is wanted).
#[derive(Default)]
pub(crate) struct TextStyles {
    faces: Vec<super::scan::FaceDef>,
    by_family: Option<HashMap<String, String>>,
    made: HashMap<Fam, HashMap<String, Option<String>>>,
    index: HashMap<Fam, StyleKeys>,
    /// The list style the save made for each row of a list, by the row's cells.
    lists: HashMap<String, String>,
    /// The style made from a style of the file for a row that changed in some of its cells, by the family, the style, the row it was made for and the row it is for:
    /// asked for each paragraph and run of the row, made once.
    derived: HashMap<(Fam, String, String, String), String>,
    /// The items a copy of a style of the part holds, by where the style starts: counted by one scan of the style, for as many copies as are made of it
    /// ([`Editor::style_items`]).
    pub(crate) items: HashMap<usize, u64>,
    /// The styles of the part (by their place among its definitions) this save edited where they stand: what they say is no longer what the file's bytes say, so the
    /// index of styles by their attributes leaves them out ([`Editor::existing_style`]).
    edited: std::collections::HashSet<usize>,
    /// The `style:font-face` elements this save adds, in the order they were made, and the (name, family) of each: put into `office:font-face-decls` at the end
    /// of the splice ([`Editor::finish_faces`]).
    new_faces: String,
    pub(crate) faces_added: Vec<(String, String)>,
    /// The names of the faces of the part, with those this save adds: made once, the first time a name is wanted, so that a name is one look.
    face_names: Option<std::collections::HashSet<String>>,
    /// The faces the cells of the paragraph being written put its own text in, as (the local name of the attribute that names the face, the family): the family a name in the text properties of its
    /// style stands for while the part declares no face of that name ([`Editor::declared_face_of`]). Set for the paragraph's own style and empty for every other style (GATE1622-CROSS-FONTNAME).
    pub(crate) own_faces: Vec<(&'static str, String)>,
    /// A paragraph was put in, taken out or made a list's: the paragraph count of the shape may differ from the file's, and the reader is asked what it reads.
    pub(crate) restructured: bool,
    /// The automatic styles the model writer made under the name the file gave them, by family and name, with what each was made from: the cells and the held values it says, and the
    /// faces its text properties name as they were written.
    pub(crate) held_made: HashMap<(Fam, String), (String, String)>,
    /// The styles made for a held style asked for again with another content (and a run's), by family, the name the file gave the style and the content, with the name each was given: one for each
    /// content, whatever the number of paragraphs that ask for it. **`None` is a content that says nothing** (no cell, no held value and nothing in the file's style that no cell reads), which is no
    /// style: it is looked at once as well.
    pub(crate) held_variants: HashMap<(Fam, String, String), Option<String>>,
    /// The names of held styles that said nothing and were made no style, by family and name, with the content each was asked for: a style asked for again, for each of a million spans, is one look.
    pub(crate) held_none: HashMap<(Fam, String), String>,
    /// The list styles the part holds, by their content without their names: a list style the model writer would make that the part has already is that style.
    pub(crate) list_index: Option<HashMap<String, String>>,
    /// The colour and font tables of the document, indexed once for the save.
    pub(crate) tables: Option<model::Tables>,
    /// The elements of the text of the shape being written that the page binds no prefix for, and that were left out or written without their attributes: the save says how many.
    pub(crate) unbound: usize,
    /// The attributes of the kept text and paragraph styles' properties that no cell reads and that this save could not write: the save says how many.
    pub(crate) style_attributes_left: usize,
}

impl TextStyles {
    pub(crate) fn new(faces: Vec<super::scan::FaceDef>) -> TextStyles {
        TextStyles { faces, ..TextStyles::default() }
    }

    /// The keys the walks of the indexes of every family went over ([`StyleKeys::retain`]).
    fn index_walks(&self) -> u64 {
        self.index.values().fold(0u64, |sum, keys| sum.saturating_add(keys.walked))
    }
}

impl Editor<'_, '_> {
    /// Carries out the plan of a text edit on the shape whose element is `shape` (on page `who.0`, shape `who.1`, for the notes), and gives what the check of the
    /// bytes made looks for: the paragraph style written for each paragraph put in (by its number in the model's text), and the zero-width elements and fields
    /// put in each paragraph written, put in, kept or edited in place.
    ///
    /// **Two passes.** The first reads what each paragraph the plan edits or takes out holds besides its characters ([`Editor::plan_paragraphs`]: how an edit is
    /// written, and the zero-width elements and fields it holds), and carries the elements over to where the new text has them, block by block
    /// ([`Editor::land_all`]); the second writes, step by step, as the plan says.
    pub(crate) fn retext(&mut self, shape: &super::scan::ScanShape, edit: &TextEdit, who: (usize, u32)) -> Result<Retexted, WriteError> {
        let kids: Vec<Kid> = self.kids.get(shape.kids.0 as usize..shape.kids.1 as usize).unwrap_or_default().to_vec();
        // The places a kid names another by are of the part's list; `kids` is the shape's own.
        let base = shape.kids.0 as usize;
        let local = |at: usize| at.checked_sub(base).and_then(|k| kids.get(k));
        let mut paras: Vec<ParaAt> = Vec::new();
        // A soft page break between paragraphs is anchored at the start of the paragraph after it (R10 item 1): the breaks before each paragraph are a range of one list.
        let mut brk: Option<usize> = None;
        let mut breaks: Vec<(usize, usize)> = Vec::new();
        let mut brk_from = 0usize;
        for kid in &kids {
            if let Kid::Break { lead, end } = kid {
                breaks
                    .try_reserve(1)
                    .map_err(|_| WriteError::Limit { what: "memory for the soft page breaks of a shape", max: breaks.len() as u64 })?;
                breaks.push((*lead, *end));
                brk.get_or_insert(*lead);
            }
            if let Kid::Para { open, empty, close_start, end, scope, lead, item, list } = kid {
                let item = (*item).and_then(local).and_then(|k| match k {
                    Kid::Item { lead, end, children, header, .. } => Some(ItemAt { lead: *lead, end: *end, alone: *children == 1, header: *header }),
                    _ => None,
                });
                let list = (*list).and_then(local).and_then(|k| match k {
                    Kid::List { lead, end } => Some((*lead, *end)),
                    _ => None,
                });
                paras.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: paras.len() as u64 })?;
                paras.push(ParaAt {
                    open: *open,
                    empty: *empty,
                    close_start: *close_start,
                    end: *end,
                    scope: *scope,
                    lead: *lead,
                    item,
                    list,
                    brk: brk.take(),
                    brks: (brk_from, breaks.len()),
                });
                brk_from = breaks.len();
            }
        }
        if paras.len() != edit.old.len() {
            return Err(WriteError::Unsupported("a shape whose paragraphs the file holds in another number than the model's"));
        }
        self.work.order_steps = self.work.order_steps.saturating_add(edit.steps.len() as u64);
        // For each step, the paragraph of the file at or after it that is kept or edited (the one an insertion is put before when none comes first).
        let mut following: Vec<Option<usize>> = vec![None; edit.steps.len() + 1];
        for (k, step) in edit.steps.iter().enumerate().rev() {
            following[k] = match *step {
                Step::Keep { old, .. } | Step::Edit { old, .. } => Some(old),
                _ => following[k + 1],
            };
        }
        let mut before: Option<usize> = None;
        // What each paragraph of the file says of the styles of its runs, and the style each names, **found once for each paragraph** however many of the model's
        // paragraphs are put beside it (a paragraph tokenized again for every neighbour it has is the cost of the edits times its size).
        let mut cache = NearCache { breaks, ..NearCache::default() };
        cache.near_inserted.try_reserve_exact(paras.len()).map_err(|_| WriteError::Limit { what: MEMORY_WORDS, max: paras.len() as u64 })?;
        cache.near_inserted.resize(paras.len(), false);
        {
            let mut prior: Option<usize> = None;
            for (k, step) in edit.steps.iter().enumerate() {
                match *step {
                    Step::Keep { old, .. } | Step::Edit { old, .. } => prior = Some(old),
                    Step::Insert { .. } => {
                        for beside in [prior, following.get(k).copied().flatten()].into_iter().flatten() {
                            if let Some(slot) = cache.near_inserted.get_mut(beside) {
                                *slot = true;
                            }
                        }
                    }
                    Step::Remove { .. } => {}
                }
            }
        }
        let Planned { mut plans, found } = self.plan_paragraphs(&paras, edit, &mut cache)?;
        let mut incoming = self.land_all(edit, &plans, &found, who)?;
        // A text that holds no element has none to put back, and a paragraph written is not checked for any.
        cache.checking = !found.is_empty();
        drop(found);
        let mut pending: Option<PendingList> = None;
        for (k, step) in edit.steps.iter().enumerate() {
            if !matches!(step, Step::Insert { .. }) {
                self.flush_list(&mut pending)?;
                cache.last = None;
            }
            match *step {
                Step::Keep { old, new } => {
                    if !incoming.is_empty() {
                        let at = self.para_at(&paras, old)?;
                        self.put_edges(&at, (old, new), &mut incoming, None, &mut cache.expect)?;
                    }
                    before = Some(old);
                }
                Step::Edit { old, new } => {
                    let (at, o, n) = (self.para_at(&paras, old)?, edit.old.get(old), edit.new.get(new));
                    let (Some(o), Some(n)) = (o, n) else { return Err(WriteError::Mismatch("a plan names a paragraph the text does not have")) };
                    let plan = plans.get_mut(k).and_then(Option::take).ok_or(WriteError::Mismatch("a plan names a paragraph that was not read"))?;
                    self.write_edit(&at, (old, new), (o, n), plan, &mut incoming, &mut cache)?;
                    before = Some(old);
                }
                Step::Remove { old } => {
                    let at = self.para_at(&paras, old)?;
                    let (lead, end) = at.unit().ok_or(WriteError::Unsupported(WORDS_ITEM))?;
                    self.work.charge_attribute(self.limits)?;
                    if let Some(book) = self.book.as_mut() {
                        book.unit_removed(lead, end, self.work);
                    }
                    self.splicer.remove(lead, end);
                    self.texts.restructured = true;
                }
                Step::Insert { new } => {
                    let n = edit.new.get(new).ok_or(WriteError::Mismatch("a plan names a paragraph the text does not have"))?;
                    let (prev, next) = (before, following.get(k).copied().flatten());
                    let mut zeros = incoming.remove(&Target::New(new)).unwrap_or_default();
                    self.insert_para(&paras, edit, (new, n, &mut zeros), (prev, next), &mut cache, &mut pending)?;
                    self.texts.restructured = true;
                }
            }
        }
        self.flush_list(&mut pending)?;
        if !incoming.is_empty() {
            return Err(WriteError::Mismatch("a zero-width element a text edit carries over was put in no paragraph"));
        }
        Ok(Retexted { styles: cache.inserted, expect: cache.expect })
    }

    fn para_at(&self, paras: &[ParaAt], at: usize) -> Result<ParaAt, WriteError> {
        paras.get(at).copied().ok_or(WriteError::Mismatch("a paragraph the model names is not in the file"))
    }

    /// The prefixes and the qualified name of the paragraph element at `at`.
    fn para_name(&mut self, at: &ParaAt) -> Result<String, WriteError> {
        self.work.charge_attribute_bytes(at.open.1.saturating_sub(at.open.0))?;
        let mut spans: Vec<AttrSpan> = Vec::new();
        let parts =
            tag_attributes(self.bytes, at.open.0, at.open.1, &mut spans).ok_or(WriteError::Mismatch("a paragraph tag the writer cannot read"))?;
        Ok(String::from_utf8_lossy(self.bytes.get(at.open.0 + 1..parts.name_end).unwrap_or_default()).into_owned())
    }

    /// The first pass: for each step of the plan, how an edited paragraph is written ([`EditPlan`]), and the zero-width elements and fields that the paragraphs the
    /// plan edits (not in place) or takes out hold, by the paragraph's number in the file, in the order they stand in.
    fn plan_paragraphs(&mut self, paras: &[ParaAt], edit: &TextEdit, cache: &mut NearCache) -> Result<Planned, WriteError> {
        let mut plans: Vec<Option<EditPlan>> = Vec::new();
        plans
            .try_reserve_exact(edit.steps.len())
            .map_err(|_| WriteError::Limit { what: "memory for the plan of a retext", max: edit.steps.len() as u64 })?;
        let mut anchors: HashMap<usize, Vec<Zero>> = HashMap::new();
        let mut seq = 0usize;
        for step in &edit.steps {
            let plan = match *step {
                Step::Edit { old, new } => {
                    let at = self.para_at(paras, old)?;
                    let (Some(o), Some(n)) = (edit.old.get(old), edit.new.get(new)) else {
                        return Err(WriteError::Mismatch("a plan names a paragraph the text does not have"));
                    };
                    // The breaks before the paragraph are given their places in the order first, so that they come before what the paragraph holds at one offset.
                    let breaks = self.break_zeros(&at, cache, &mut seq)?;
                    let plan = self.plan_edit(&at, old, (o, n), cache, (&mut anchors, &mut seq))?;
                    // A paragraph kept or edited in place keeps its breaks where they stand; one written whole carries them over with its block (R10 item 3).
                    if matches!(plan, EditPlan::Whole { .. }) {
                        self.anchor_breaks(old, breaks, &mut anchors)?;
                    }
                    Some(plan)
                }
                Step::Remove { old } => {
                    let at = self.para_at(paras, old)?;
                    let Some(o) = edit.old.get(old) else { return Err(WriteError::Mismatch("a plan names a paragraph the text does not have")) };
                    let breaks = self.break_zeros(&at, cache, &mut seq)?;
                    self.removed_zero(&at, old, o, (&mut anchors, &mut seq))?;
                    self.anchor_breaks(old, breaks, &mut anchors)?;
                    None
                }
                _ => None,
            };
            plans.push(plan);
        }
        Ok(Planned { plans, found: anchors })
    }

    /// Reads the paragraph at `at`, which the baseline has as `old` and the model as `new`: how it is written ([`EditPlan`]) and, for a paragraph written whole,
    /// the zero-width elements and fields it holds (added to `found` under `ix`). A paragraph that holds what the writer does not map is refused in words.
    fn plan_edit(
        &mut self,
        at: &ParaAt,
        ix: usize,
        (old, new): (&View, &View),
        cache: &mut NearCache,
        (found, seq): (&mut HashMap<usize, Vec<Zero>>, &mut usize),
    ) -> Result<EditPlan, WriteError> {
        let pfx = Pfx::of(&mut self.scopes, at.scope, self.work);
        self.work.charge_copy(new.chars.len(), self.limits)?;
        let content = if at.empty { (at.open.1, at.open.1) } else { (at.open.1, at.close_start) };
        // The content is tokenized: charged before it is.
        self.work.charge_tokens(content.1.saturating_sub(content.0), self.limits)?;
        let ns_of = |n: &str| -> Option<&'static str> {
            let (prefix, local) = n.split_once(':')?;
            (prefix == pfx.text).then(|| local_name(local))
        };
        let Some((tokens, structure)) = tokenize(self.bytes, content.0, content.1, &ns_of) else {
            let bytes = self.bytes.get(content.0..content.1).ok_or(WriteError::Mismatch(WORDS_NOT_THERE))?;
            return Err(WriteError::Unsupported(if holds_tag(bytes, &pfx.text, |local| local == "a")? { WORDS_LINK } else { WORDS_UNMAPPED }));
        };
        // The tokens are held (the token of each piece and the characters of each text, four bytes each) until the paragraph is planned: charged to the memory a
        // save may hold, and given back when it is.
        let held = tokens_bytes(&tokens);
        self.work.hold(held, MEMORY_WORDS)?;
        let planned = self.plan_tokens((at, ix), (old, new), cache, (found, seq), (&tokens, structure), &pfx);
        self.work.release(held);
        planned
    }

    /// The rest of [`Editor::plan_edit`], with the paragraph's `tokens` read: how the paragraph is written, and the elements and fields it holds.
    fn plan_tokens(
        &mut self,
        (at, ix): (&ParaAt, usize),
        (old, new): (&View, &View),
        cache: &mut NearCache,
        (found, seq): (&mut HashMap<usize, Vec<Zero>>, &mut usize),
        (tokens, structure): (&[Token], bool),
        pfx: &Pfx,
    ) -> Result<EditPlan, WriteError> {
        // What the paragraph stands for is counted first, and built only up to what the model says it is (a count of spaces is never expanded into more than
        // the paragraph the model holds). The file is the one the model was read from, so a paragraph that does not read as the model's text did is one the
        // writer reads another way than the reader does, and is refused as a case it does not map.
        let wanted = old.chars.chars().count();
        let read = reading(tokens, wanted, true).filter(|r| r.chars == old.chars);
        let Some(read) = read else {
            return Err(WriteError::Unsupported(
                "a paragraph whose white space or counted spaces the writer does not read as the reader read them (its text is not rewritten)",
            ));
        };
        self.work.charge_attribute(self.limits)?;
        let same_runs =
            old.pp_same(new) && old.runs.len() == new.runs.len() && old.runs.iter().zip(&new.runs).all(|(a, b)| same_key(&a.props, &b.props));
        // The runs and the row are the same: where the edit lies inside one run of raw text, only that text is replaced, and every element stays (R10 item 2).
        if structure
            && same_runs
            && let Some((from, to, text)) = Self::in_place(self.bytes, tokens, old, new, pfx)
        {
            return Ok(EditPlan::InPlace { from, to, text });
        }
        if !old.pp_same(new) && at.item.is_some() {
            return Err(WriteError::Unsupported(WORDS_LIST_CHANGED));
        }
        // The styles the paragraph's own spans name for its runs are used again for the runs of the same row (and kept for the paragraphs put beside it).
        // The style of each run's span, found once (a step for each run, each token and each segment looked at): it is what the runs of the same row use again, and
        // what the runs whose rows change are made from. A slot for each run is charged to the memory a save may hold (kept for the shape's retext, as the spans kept
        // are).
        self.work.order_steps = self.work.order_steps.saturating_add(old.runs.len() as u64).saturating_add(tokens.len() as u64);
        let names = run_style_names(tokens, &old.runs, &mut self.work.order_steps)?;
        let mut own: HashMap<String, String> = HashMap::new();
        reuse_from(&old.runs, &names, &mut own);
        cache.reuse.insert(ix, Rc::new(own));
        let held = names.iter().fold(0u64, |sum, n| sum.saturating_add(RUN_STYLE_BYTES).saturating_add(n.as_ref().map_or(0, String::len) as u64));
        let cap = self.work.memory_cap;
        self.work.memory_room = self.work.memory_room.checked_sub(held).ok_or(WriteError::Limit { what: MEMORY_WORDS, max: cap })?;
        cache.run_styles.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: ix as u64 })?;
        cache.run_styles.insert(ix, Rc::new(names));
        let kept = if structure { self.kept_runs(tokens, old, new) } else { HashMap::new() };
        // The spans kept are held until the second pass writes the paragraph: charged to the memory a save may hold.
        let held = (kept.len() as u64).saturating_mul(KEPT_BYTES);
        let cap = self.work.memory_cap;
        self.work.memory_room = self.work.memory_room.checked_sub(held).ok_or(WriteError::Limit { what: MEMORY_WORDS, max: cap })?;
        self.collect_zero(tokens, &read, (at.scope, ix), found, seq)?;
        Ok(EditPlan::Whole { kept })
    }

    /// The elements of a paragraph the plan takes out, in `found` under `ix`. A paragraph that holds none is not read.
    fn removed_zero(
        &mut self,
        at: &ParaAt,
        ix: usize,
        old: &View,
        (found, seq): (&mut HashMap<usize, Vec<Zero>>, &mut usize),
    ) -> Result<(), WriteError> {
        let content = if at.empty { (at.open.1, at.open.1) } else { (at.open.1, at.close_start) };
        // A paragraph the scan found is in the part: a range that is not is a fault of the scan, and not a paragraph with no element in it.
        let bytes = self.bytes.get(content.0..content.1).ok_or(WriteError::Mismatch(WORDS_NOT_THERE))?;
        // An element is a tag: a paragraph with none holds none (the bytes looked at are counted).
        self.work.zero_steps = self.work.zero_steps.saturating_add(bytes.len() as u64);
        if !bytes.contains(&b'<') {
            return Ok(());
        }
        let pfx = Pfx::of(&mut self.scopes, at.scope, self.work);
        let ns_of = |n: &str| -> Option<&'static str> {
            let (prefix, local) = n.split_once(':')?;
            (prefix == pfx.text).then(|| local_name(local))
        };
        self.work.charge_tokens(content.1.saturating_sub(content.0), self.limits)?;
        let holds = holds_tag(bytes, &pfx.text, |local| zero::mark_name(local).is_some() || zero::field_name(local).is_some())?;
        // What the writer does not map, in a paragraph that holds no element to put elsewhere, goes with the paragraph, as it did.
        let Some((tokens, _)) = tokenize(self.bytes, content.0, content.1, &ns_of) else {
            return if holds { Err(WriteError::Unsupported(WORDS_UNMAPPED)) } else { Ok(()) };
        };
        // The tokens are held until the paragraph's elements are listed, and given back (see `plan_edit`).
        let held = tokens_bytes(&tokens);
        self.work.hold(held, MEMORY_WORDS)?;
        let listed = self.list_removed(&tokens, old, (at.scope, ix), (found, seq), holds);
        self.work.release(held);
        listed
    }

    /// The rest of [`Editor::removed_zero`], with the paragraph's `tokens` read: its elements and fields are listed when it reads as the baseline's text. `holds`:
    /// the paragraph holds an element to put elsewhere, so that a paragraph that does not read so is refused and not taken out with its elements lost.
    fn list_removed(
        &mut self,
        tokens: &[Token],
        old: &View,
        (scope, ix): (u32, usize),
        (found, seq): (&mut HashMap<usize, Vec<Zero>>, &mut usize),
        holds: bool,
    ) -> Result<(), WriteError> {
        let wanted = old.chars.chars().count();
        let Some(read) = reading(tokens, wanted, true).filter(|r| r.chars == old.chars) else {
            return if holds {
                Err(WriteError::Unsupported(
                    "a paragraph with a zero-width element whose white space the writer does not read as the reader read it (its elements are not put elsewhere)",
                ))
            } else {
                Ok(())
            };
        };
        self.collect_zero(tokens, &read, (scope, ix), found, seq)
    }

    /// The soft page breaks that stand between paragraphs right before the paragraph `at`, as anchors at its start (R10 item 1), with their places in the order of
    /// the text taken from `seq` (before the paragraph's own elements, which they stood before). Nothing is changed in the file.
    fn break_zeros(&mut self, at: &ParaAt, cache: &NearCache, seq: &mut usize) -> Result<Vec<Zero>, WriteError> {
        let range =
            cache.breaks.get(at.brks.0..at.brks.1).ok_or(WriteError::Mismatch("the soft page breaks of a paragraph are not in the shape's list"))?;
        let mut out: Vec<Zero> = Vec::new();
        out.try_reserve_exact(range.len()).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
        for &(lead, end) in range {
            out.push(Zero { bytes: (lead, end), at: 0, len: 0, field: None, scope: at.scope, seq: *seq, between: true, shown: None });
            *seq = seq.saturating_add(1);
        }
        Ok(out)
    }

    /// Makes `breaks` the anchors of the paragraph numbered `ix` of the file, which the plan writes whole or takes out: charged to the memory a save may hold as any
    /// element is, taken out of the file where they stand, and put again where the block's mapping says (R10 item 3).
    fn anchor_breaks(&mut self, ix: usize, breaks: Vec<Zero>, found: &mut HashMap<usize, Vec<Zero>>) -> Result<(), WriteError> {
        if breaks.is_empty() {
            return Ok(());
        }
        let need = (breaks.len() as u64).saturating_mul(zero::ELEMENT_BYTES);
        let cap = self.work.memory_cap;
        self.work.memory_room = self.work.memory_room.checked_sub(need).ok_or(WriteError::Limit { what: zero::MEMORY_WORDS, max: cap })?;
        self.work.zero_steps = self.work.zero_steps.saturating_add(breaks.len() as u64);
        let list = found.entry(ix).or_default();
        list.try_reserve(breaks.len()).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: cap })?;
        for z in breaks {
            self.work.charge_attribute(self.limits)?;
            self.splicer.remove(z.bytes.0, z.bytes.1);
            list.push(z);
        }
        Ok(())
    }

    /// Takes out of `zeros` the soft page breaks that stood between paragraphs and land on the start of a paragraph, and gives their bytes as they stood, in order, white
    /// space before each included: they are written between the paragraphs, or at the start of the paragraph's list item (R10 item 5), and not inside the paragraph. A
    /// prefix in their bytes is the one in force where they stood, so they are put only where the same bindings are (`scope`).
    fn take_between(&mut self, zeros: &mut Vec<Zero>, scope: u32) -> Result<String, WriteError> {
        // The test is one function, so that the quick answer for a paragraph that has none and the partition cannot disagree.
        let lands_between = |z: &Zero| z.between && z.at == 0;
        if !zeros.iter().any(lands_between) {
            return Ok(String::new());
        }
        let (mut between, mut rest): (Vec<Zero>, Vec<Zero>) = (Vec::new(), Vec::new());
        rest.try_reserve_exact(zeros.len()).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
        for z in zeros.drain(..) {
            if lands_between(&z) {
                between.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
                between.push(z);
            } else {
                rest.push(z);
            }
        }
        *zeros = rest;
        between.sort_by_key(|z| z.seq);
        let mut text = String::new();
        for z in &between {
            if z.scope != scope {
                return Err(WriteError::Unsupported(WORDS_ZERO_SCOPE));
            }
            let one = self
                .bytes
                .get(z.bytes.0..z.bytes.1)
                .and_then(|b| std::str::from_utf8(b).ok())
                .ok_or(WriteError::Mismatch("a soft page break's bytes are not in the part"))?;
            self.work.charge_copy(one.len(), self.limits)?;
            self.work.zero_steps = self.work.zero_steps.saturating_add(1);
            text.push_str(one);
        }
        Ok(text)
    }

    /// Where the soft page breaks that land on the start of the paragraph `at` are written (R10 item 5): between it and the paragraph before it, before any break that
    /// stands there; at the start of its list item when it is a list item's, since a list holds no soft page break between its items.
    fn before_paragraph(at: &ParaAt) -> usize {
        if at.item.is_some() { at.lead } else { at.before(at.lead) }
    }

    /// Lists the elements and fields among the tokens of the paragraph numbered `ix` of the file, where each stood (`read`): charged to the memory a save may hold
    /// before they are listed.
    fn collect_zero(
        &mut self,
        tokens: &[Token],
        read: &Reading,
        (scope, ix): (u32, usize),
        found: &mut HashMap<usize, Vec<Zero>>,
        seq: &mut usize,
    ) -> Result<(), WriteError> {
        if read.placed.is_empty() {
            return Ok(());
        }
        let need = (read.placed.len() as u64).saturating_mul(zero::ELEMENT_BYTES);
        let cap = self.work.memory_cap;
        self.work.memory_room = self.work.memory_room.checked_sub(need).ok_or(WriteError::Limit { what: zero::MEMORY_WORDS, max: cap })?;
        self.work.zero_steps = self.work.zero_steps.saturating_add(read.placed.len() as u64);
        let list = found.entry(ix).or_default();
        list.try_reserve_exact(read.placed.len()).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: cap })?;
        for &(t, offset, len) in &read.placed {
            let (bytes, field, len) = match tokens.get(t) {
                Some(Token::Mark { bytes }) => (*bytes, None, 0),
                Some(Token::Field { bytes, kind, .. }) => (*bytes, Some(*kind), len),
                _ => return Err(WriteError::Mismatch("a zero-width element the reading placed is not among the tokens")),
            };
            list.push(Zero { bytes, at: offset, len, field, scope, seq: *seq, between: false, shown: None });
            *seq = seq.saturating_add(1);
        }
        Ok(())
    }

    /// Carries the elements found over to where the new text has them, block by block (R10 item 3 and 4): the paragraphs the plan keeps and those it edits in place
    /// keep their own, and between two of them the steps that edit (not in place), put in or take out make a block. The elements that land in each paragraph are
    /// given by where they land ([`Target`]), in the order they stood in. A field whose characters changed is written as the new characters, and the save says so.
    fn land_all(
        &mut self,
        edit: &TextEdit,
        plans: &[Option<EditPlan>],
        anchors: &HashMap<usize, Vec<Zero>>,
        who: (usize, u32),
    ) -> Result<HashMap<Target, Vec<Zero>>, WriteError> {
        let mut incoming: HashMap<Target, Vec<Zero>> = HashMap::new();
        if anchors.is_empty() {
            return Ok(incoming);
        }
        let steps = &edit.steps;
        // A paragraph the plan keeps, or edits in place, and its number in the file.
        let boundary = |k: usize| -> Option<usize> {
            match (steps.get(k)?, plans.get(k)) {
                (Step::Keep { old, .. }, _) => Some(*old),
                (Step::Edit { old, .. }, Some(Some(EditPlan::InPlace { .. }))) => Some(*old),
                _ => None,
            }
        };
        let mut landed: Vec<Landed> = Vec::new();
        let mut plain: Vec<&'static str> = Vec::new();
        let (mut k, mut before) = (0usize, None);
        while k < steps.len() {
            if let Some(old) = boundary(k) {
                before = Some(old);
                k += 1;
                continue;
            }
            // The block: the steps up to the next paragraph kept, and whether it takes out or rewrites a paragraph that holds an element.
            let mut end = k;
            let mut holds = false;
            while end < steps.len() && boundary(end).is_none() {
                if let Some(Step::Edit { old, .. } | Step::Remove { old }) = steps.get(end) {
                    holds |= anchors.contains_key(old);
                }
                self.work.order_steps = self.work.order_steps.saturating_add(1);
                end += 1;
            }
            if holds {
                let mut old_side: Vec<(usize, &str)> = Vec::new();
                let mut new_side: Vec<(usize, &str)> = Vec::new();
                let mut new_fields: Vec<&[zero::FieldAt]> = Vec::new();
                // A block of steps that is not in the plan is a fault, and not a block with no paragraph in it: the elements of its paragraphs would be lost.
                let block_steps = steps.get(k..end).ok_or(WriteError::Mismatch("a block of the plan that is not in the plan"))?;
                for step in block_steps {
                    if let Step::Edit { old, .. } | Step::Remove { old } = *step {
                        old_side.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: old as u64 })?;
                        old_side.push((old, edit.old.get(old).map_or("", |v| v.chars.as_str())));
                    }
                    if let Step::Edit { new, .. } | Step::Insert { new } = *step {
                        new_side.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: new as u64 })?;
                        new_fields.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: new as u64 })?;
                        new_side.push((new, edit.new.get(new).map_or("", |v| v.chars.as_str())));
                        new_fields.push(edit.new.get(new).map_or(&[], |v| v.fields.as_slice()));
                    }
                }
                let after = boundary(end);
                let block = zero::Block { old: &old_side, new: &new_side, new_fields: &new_fields, before, after };
                let mut compared = 0u64;
                zero::land(&block, anchors, &mut compared, &mut landed, &mut plain).map_err(WriteError::Unsupported)?;
                self.work.zero_steps = self.work.zero_steps.saturating_add(compared);
            }
            k = end;
        }
        incoming
            .try_reserve(landed.len().min(edit.steps.len().saturating_add(1)))
            .map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
        for l in landed {
            let list = incoming.entry(l.target).or_default();
            list.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
            list.push(l.zero);
        }
        // One note for each kind of field the shape had written as plain text, and not one for each field: a text can hold a million of them, and the note says the
        // same for all. The kinds are the standard's names for a field (the table of `zero::FIELDS`), so a shape has at most
        // that many notes.
        let mut said: Vec<&'static str> = Vec::new();
        for element in plain {
            if said.contains(&element) {
                continue;
            }
            said.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the notes of a save", max: 0 })?;
            said.push(element);
            self.notes.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the notes of a save", max: 0 })?;
            self.notes.push(SaveNote::FieldWritten { page: who.0, shape: who.1, element });
        }
        Ok(incoming)
    }

    /// Writes the paragraph at `at`, whose number in the file is `ix.0` and in the model `ix.1`, as the plan of the first pass says: `true` when it was an edit in
    /// place (the paragraph keeps its bytes but for it, and takes the elements that land at its start and end).
    fn write_edit(
        &mut self,
        at: &ParaAt,
        (ix, new_ix): (usize, usize),
        (old, new): (&View, &View),
        plan: EditPlan,
        incoming: &mut HashMap<Target, Vec<Zero>>,
        cache: &mut NearCache,
    ) -> Result<(), WriteError> {
        let kept = match plan {
            EditPlan::InPlace { from, to, text } => return self.put_edges(at, (ix, new_ix), incoming, Some((from, to, text)), &mut cache.expect),
            EditPlan::Whole { kept } => kept,
        };
        let pfx = Pfx::of(&mut self.scopes, at.scope, self.work);
        let content = if at.empty { (at.open.1, at.open.1) } else { (at.open.1, at.close_start) };
        let name = self.para_name(at)?;
        let own = cache.reuse.get(&ix).map(Rc::clone).ok_or(WriteError::Mismatch("a paragraph written whole was not read"))?;
        // The content is written again, from the model's runs; the references inside what goes are let go.
        let mut zeros = incoming.remove(&Target::New(new_ix)).unwrap_or_default();
        let breaks = self.take_between(&mut zeros, at.scope)?;
        if !breaks.is_empty() {
            self.splicer.insert(Self::before_paragraph(at), breaks);
        }
        // The runs whose rows changed take their styles from the styles the file's spans name for the runs they stand in the place of (a value is kept while
        // its own cells are unchanged).
        let styles = cache.run_styles.get(&ix).map(Rc::clone).unwrap_or_default();
        let mut base = Base::new(old, new, &styles);
        // A paragraph a paragraph put in is beside takes its styles as the file has them, so its own are copied and not edited where they stand.
        let beside_new = cache.near_inserted.get(ix).copied().unwrap_or(false);
        if !beside_new {
            base.in_place = self.in_place_styles(new, &own, &base)?;
        }
        let body = if zeros.is_empty() {
            if cache.checking {
                cache.expect.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
                cache.expect.push(zero::Expect::Exact { index: new_ix, items: Vec::new() });
            }
            self.content_of(new, &pfx.text, &pfx.declare, &[&own], (&kept, Some(&base)))?
        } else {
            let (body, items) = self.content_with_zeros(new, &mut zeros, (&pfx.text, &pfx.declare), (&[&own], Some(&base)), at.scope)?;
            cache.expect.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
            cache.expect.push(zero::Expect::Exact { index: new_ix, items });
            body
        };
        if let Some(book) = self.book.as_mut() {
            book.unit_removed(content.0, content.1, self.work);
        }
        self.put_content(at.empty, at.open, content, &name, body);
        if !old.pp_same(new) {
            let style = match self.derived_paragraph_style(at, old, new, !beside_new)? {
                Some(style) => Some(style),
                None => self.paragraph_style(&new.pp)?,
            };
            self.set_style_name(at.open, at.scope, style.as_deref(), Fam::Paragraph)?;
            // A paragraph that was no list's and is given a list's row is the item of a list this save makes around it.
            if let Some(props) = new.pp.as_ref().filter(|p| p.bullet) {
                let name = self.list_style(props)?;
                self.work.charge_attribute(self.limits)?;
                let t = &pfx.text;
                self.splicer.insert(
                    at.lead,
                    format!("<{t}:list{} {t}:style-name=\"{}\"><{t}:list-item>", pfx.declare, super::splice::escape_value(&name, b'"')),
                );
                self.splicer.insert(at.end, format!("</{t}:list-item></{t}:list>"));
                self.texts.restructured = true;
            }
        }
        Ok(())
    }

    /// The text of the elements `zeros` as they are put in a paragraph: each element's own bytes, copied. A prefix in their bytes is the one in force where they
    /// stood, so they are put only where the same bindings are (`scope`).
    fn zero_bytes(&mut self, zeros: &[Zero], scope: u32) -> Result<(String, Vec<String>), WriteError> {
        let (mut text, mut each) = (String::new(), Vec::new());
        for z in zeros {
            if z.scope != scope {
                return Err(WriteError::Unsupported(WORDS_ZERO_SCOPE));
            }
            let one = self
                .bytes
                .get(z.bytes.0..z.bytes.1)
                .and_then(|b| std::str::from_utf8(b).ok())
                .ok_or(WriteError::Mismatch("a zero-width element's bytes are not in the part"))?;
            // A soft page break that stood between paragraphs begins with the white space before it; inside a paragraph it is the element alone.
            let one = if z.between { one.trim_start_matches([' ', '\t', '\r', '\n']) } else { one };
            // A field whose characters a refresh changed: its start and end tags as the file had them, around the characters it shows now.
            let one = match &z.shown {
                Some(shown) => {
                    let rewritten = refreshed_field(one, shown)
                        .ok_or(WriteError::Mismatch("a field's element has no start and end tag to write its characters between"))?;
                    std::borrow::Cow::Owned(rewritten)
                }
                None => std::borrow::Cow::Borrowed(one),
            };
            self.work.charge_copy(one.len(), self.limits)?;
            self.work.zero_steps = self.work.zero_steps.saturating_add(1);
            each.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
            text.push_str(&one);
            each.push(one.into_owned());
        }
        Ok((text, each))
    }

    /// Puts the elements that land at the start and at the end of the paragraph at `at`, which the plan keeps or edits in place (`splice` is the edit in place:
    /// the bytes it replaces and its text), into its bytes: at the start of its content and at the end of it, with the edit between (the paragraph's own bytes
    /// stay, R10 item 2 and 3). `new_ix` is its number in the model: what the check of the bytes made looks for.
    fn put_edges(
        &mut self,
        at: &ParaAt,
        (ix, new_ix): (usize, usize),
        incoming: &mut HashMap<Target, Vec<Zero>>,
        splice: Option<(usize, usize, String)>,
        expect: &mut Vec<zero::Expect>,
    ) -> Result<(), WriteError> {
        let (start, end) = (incoming.remove(&Target::Start(ix)), incoming.remove(&Target::End(ix)));
        let (mut start, mut end) = (start.unwrap_or_default(), end.unwrap_or_default());
        // The breaks that stood between paragraphs and land on this paragraph's start go before it, or at the start of its list item (R10 item 5).
        let breaks = self.take_between(&mut start, at.scope)?;
        if !breaks.is_empty() {
            self.splicer.insert(Self::before_paragraph(at), breaks);
        }
        start.sort_by_key(|z| z.seq);
        end.sort_by_key(|z| z.seq);
        let (head, first) = self.zero_bytes(&start, at.scope)?;
        let (tail, last) = self.zero_bytes(&end, at.scope)?;
        let content = if at.empty { (at.open.1, at.open.1) } else { (at.open.1, at.close_start) };
        if at.empty {
            // A paragraph written empty has no content to put into: it is opened (an empty paragraph is no edit in place).
            if splice.is_some() {
                return Err(WriteError::Mismatch("an empty paragraph is edited in place"));
            }
            if !head.is_empty() || !tail.is_empty() {
                let name = self.para_name(at)?;
                self.splicer.replace(at.open.1.saturating_sub(2), at.open.1, format!(">{head}{tail}</{name}>"));
            }
        } else {
            if !head.is_empty() {
                self.splicer.insert(content.0, head);
            }
            if let Some((from, to, text)) = splice {
                self.splicer.replace(from, to, text);
            }
            if !tail.is_empty() {
                self.splicer.insert(content.1, tail);
            }
        }
        if !first.is_empty() || !last.is_empty() {
            expect.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
            expect.push(zero::Expect::Edges { index: new_ix, start: first, end: last });
        }
        Ok(())
    }

    /// The edit of one run of raw text of the paragraph, in the paragraph's own bytes: the bytes it replaces (from, to) and what it puts there, when the edit is
    /// one the paragraph's bytes map one to one; `None` when it is not (the paragraph is then written again whole). The elements of the paragraph stay where
    /// they are (R10 item 2).
    fn in_place(bytes: &[u8], tokens: &[Token], old: &View, new: &View, pfx: &Pfx) -> Option<(usize, usize, String)> {
        // The head and the tail the two share, in characters, found by walking both without building either as a list of characters.
        let (old_len, new_len) = (old.chars.chars().count(), new.chars.chars().count());
        let head = old.chars.chars().zip(new.chars.chars()).take_while(|(x, y)| x == y).count();
        let tail = old.chars.chars().rev().zip(new.chars.chars().rev()).take(old_len.min(new_len) - head).take_while(|(x, y)| x == y).count();
        let (from, to) = (head, old_len - tail);
        let byte_of = |s: &str, k: usize| s.char_indices().nth(k).map_or(s.len(), |(b, _)| b);
        let with = new.chars.get(byte_of(&new.chars, head)..byte_of(&new.chars, new_len - tail)).unwrap_or_default().to_owned();
        let delta = new_len as isize - old_len as isize;
        // The edit does not cross a boundary of a run, or lie at one, and the boundaries the model has now are those it had, moved by what the edit added.
        let bounds = |v: &View| -> Vec<usize> { v.runs.iter().map(|r| r.start).filter(|b| *b > 0).collect() };
        let (before, now) = (bounds(old), bounds(new));
        if before.iter().any(|&b| (from < b && b < to) || (from == to && b == from)) {
            return None;
        }
        let expected: Vec<usize> = before.iter().map(|&b| if b >= to { (b as isize + delta) as usize } else { b }).collect();
        if expected != now {
            return None;
        }
        // The span form: the replaced characters lie in one run of raw text, one character for one byte range. **Every text of the paragraph reads one character
        // for each it holds** (white space that collapses, a space the reader drops at the start or beside another, would put the characters after it off by
        // what it takes: the paragraph is then written whole), so the place of a character in a text is its offset in the paragraph as the reader counts.
        let mut char_at = 0usize;
        let mut state = START;
        let mut target: Option<(usize, usize, usize, usize, State)> = None;
        for (at_token, t) in tokens.iter().enumerate() {
            match t {
                Token::Text { span, chars, reference } => {
                    // A reference stands for a character in more than one byte: the characters and the bytes are not one to one.
                    if *reference {
                        return None;
                    }
                    let before = state;
                    if walk(chars, &mut state) != chars.len() {
                        return None;
                    }
                    // An insertion at the start of a node is at a boundary, unless the node is the paragraph's first thing: then it is the start of the
                    // paragraph and nothing is before it.
                    let at_start = from == 0 && at_token == 0;
                    if from >= char_at && to <= char_at + chars.len() && (from < to || from > char_at || at_start) {
                        target = target.or(Some((span.0, span.1, char_at, at_token, before)));
                    }
                    char_at += chars.len();
                }
                // A field's characters are counted as they are written, which is as they read only when they hold no white space the reader collapses.
                Token::Field { chars, .. } => {
                    if walk(chars, &mut state) != chars.len() {
                        return None;
                    }
                    char_at += chars.len();
                }
                Token::Element(space) => {
                    char_at += space.len();
                    state = (false, false);
                }
                Token::SpanOpen { .. } | Token::SpanClose { .. } | Token::Mark { .. } => {}
            }
        }
        let (node_from, node_to, start_char, at_node, before) = target?;
        // A node that is not in the part, or is not text, is no node to edit in place: the paragraph is written whole, and no text is made of nothing.
        let node = bytes.get(node_from..node_to).and_then(|b| std::str::from_utf8(b).ok())?;
        // The characters after the node read as they did: the node ends in the state it ended in. What the edit puts is read the same wherever it stands (a first
        // or last space of it is `text:s`), and what is left of the node on each side of it reads as it did unless the edit takes the character before it.
        let Some(Token::Text { chars: held, .. }) = tokens.get(at_node) else { return None };
        let (a_chars, b_chars) = (from - start_char, to - start_char);
        let (head_chars, tail_chars) = (held.get(..a_chars)?, held.get(b_chars..)?);
        let mut was = before;
        walk(held, &mut was);
        let mut now = before;
        if walk(head_chars, &mut now) != head_chars.len() {
            return None;
        }
        if !with.is_empty() {
            now = (false, false);
        }
        if walk(tail_chars, &mut now) != tail_chars.len() || now != was {
            return None;
        }
        // Bytes of the characters before and after the edit inside the node.
        let at_byte = |k: usize| node.char_indices().nth(k).map_or(node.len(), |(b, _)| b);
        let (a, mut b) = (at_byte(from - start_char), at_byte(to - start_char));
        // **The white space that ends the paragraph goes with the characters the edit replaces when the edit reaches the paragraph's last one** (the milestone-1 rerun's item 3:
        // a span's trailing space stayed after the new text, invisible, and the next read of it was right). The reader drops a collapsed space at the paragraph's end, so the
        // characters the model has stop before it, and what the node holds after them is that space and nothing else: the bytes the edit replaces go on to the node's end.
        // Only where nothing but the ends of spans follows the node (no element, no text, no field).
        if to == old_len && tokens.iter().skip(at_node + 1).all(|t| matches!(t, Token::SpanClose { .. })) {
            b = node.len();
        }
        let text = encode(&with, &pfx.text, &pfx.declare);
        Some((node_from + a, node_from + b, text))
    }

    /// The runs of the new paragraph that are the old paragraph's, whose `text:span` the file has: a run is unchanged when it lies in the head or the tail the two
    /// paragraphs have in common, with the same characters, the same place (the tail shifted by what was added) and the same row, and its span is one element
    /// that covers it and no more (an unchanged run keeps its `text:span`, with its `xml:id` and whatever else it holds). By the place of the run in the new
    /// paragraph. A step for each run looked at.
    fn kept_runs(&mut self, tokens: &[Token], old: &View, new: &View) -> HashMap<usize, KeptSpan> {
        let mut kept: HashMap<usize, KeptSpan> = HashMap::new();
        let Some(total) = old.runs.last().map(|r| r.end) else { return kept };
        let Some(spans) = plain_spans(tokens, total) else { return kept };
        self.work.order_steps = self.work.order_steps.saturating_add(spans.len() as u64);
        let mut by_place: HashMap<(usize, usize), KeptSpan> = HashMap::new();
        if by_place.try_reserve(spans.len()).is_err() {
            return kept;
        }
        for (first, end, span) in spans {
            by_place.insert((first, end), span);
        }
        let (old_len, new_len) = (old.chars.chars().count(), new.chars.chars().count());
        let head = old.chars.chars().zip(new.chars.chars()).take_while(|(a, b)| a == b).count();
        let tail =
            old.chars.chars().rev().zip(new.chars.chars().rev()).take(old_len.min(new_len).saturating_sub(head)).take_while(|(a, b)| a == b).count();
        let same_row = |a: &runs::Run, b: &runs::Run| a.props.as_deref().map(|p| p.key.as_str()) == b.props.as_deref().map(|p| p.key.as_str());
        let mut from_head = 0usize;
        while let (Some(o), Some(n)) = (old.runs.get(from_head), new.runs.get(from_head)) {
            self.work.order_steps = self.work.order_steps.saturating_add(1);
            if o.end > head || (o.start, o.end) != (n.start, n.end) || !same_row(o, n) {
                break;
            }
            if let Some(span) = by_place.get(&(o.start, o.end)) {
                kept.insert(from_head, span.clone());
            }
            from_head += 1;
        }
        let shift = new_len as isize - old_len as isize;
        let (mut o_at, mut n_at) = (old.runs.len(), new.runs.len());
        while o_at > from_head && n_at > from_head {
            self.work.order_steps = self.work.order_steps.saturating_add(1);
            let (Some(o), Some(n)) = (old.runs.get(o_at - 1), new.runs.get(n_at - 1)) else { break };
            let moved = |x: usize| (x as isize + shift) as usize;
            if o.start + tail < old_len || moved(o.start) != n.start || moved(o.end) != n.end || !same_row(o, n) {
                break;
            }
            if let Some(span) = by_place.get(&(o.start, o.end)) {
                kept.insert(n_at - 1, span.clone());
            }
            o_at -= 1;
            n_at -= 1;
        }
        kept
    }

    /// The content that reads as the paragraph `view`: each run with a style as a `text:span` of its automatic text style, the others raw text. A run in `kept` is
    /// the file's own `text:span`, as it is.
    fn content_of(
        &mut self,
        view: &View,
        text_prefix: &str,
        declare: &str,
        reuse: &[&HashMap<String, String>],
        (kept, base): (&HashMap<usize, KeptSpan>, Option<&Base<'_>>),
    ) -> Result<String, WriteError> {
        let mut out = String::new();
        let styled = !view.runs.is_empty();
        if !styled {
            return Ok(encode(&view.chars, text_prefix, declare));
        }
        // One pass over the characters: each run takes its share from the iterator.
        let mut chars = view.chars.char_indices().peekable();
        for (at, run) in view.runs.iter().enumerate() {
            let start = chars.peek().map_or(view.chars.len(), |(b, _)| *b);
            for _ in run.start..run.end {
                chars.next();
            }
            let end = chars.peek().map_or(view.chars.len(), |(b, _)| *b);
            let segment = view.chars.get(start..end).unwrap_or_default();
            self.work.order_steps = self.work.order_steps.saturating_add(1);
            if let Some(span) = kept.get(&at)
                && let Some(original) = self.bytes.get(span.bytes.0..span.bytes.1).and_then(|b| std::str::from_utf8(b).ok())
            {
                self.work.charge_copy(original.len(), self.limits)?;
                out.push_str(original);
                // The style it names is named once more by the element that stays.
                if let (Some(name), Some(book)) = (&span.style, self.book.as_mut())
                    && let Some(i) = book.find_def(name, Fam::Text)
                {
                    book.add_reference(i);
                }
                continue;
            }
            let body = encode(segment, text_prefix, declare);
            match run.props.as_deref().filter(|p| p.writes()) {
                Some(props) => {
                    let style = self.run_style(props, reuse, base.map(|b| (b, run.start)))?;
                    self.work.charge_attribute(self.limits)?;
                    out.push_str(&format!(
                        "<{text_prefix}:span{declare} {text_prefix}:style-name=\"{}\">{body}</{text_prefix}:span>",
                        super::splice::escape_value(&style, b'"')
                    ));
                }
                None => out.push_str(&body),
            }
        }
        Ok(out)
    }

    /// The content that reads as the paragraph `view` and holds the elements `zeros` (R10 item 5 and 6): each run with a style as a `text:span` of its automatic
    /// text style, the others raw text, and each element at the offset it has: **at the paragraph's top level** when the offset is the paragraph's start or end, or
    /// lies between two runs (before the next run's span), **inside a run's `text:span`**, splitting its text there, when the characters on both sides belong to
    /// the one run, and a field inside the run whose characters it shows (a field the new runs cut is refused). The characters are written in one pass
    /// ([`zero::Spaces`]): a space is `text:s` where a plain one would not read back, so both forms of the collapsing algorithm read the spaces the model holds.
    /// No `text:span` of the file is kept as its bytes here (a span that held an element has none of it to keep). Gives the content and, for the check of the
    /// bytes made, each element as `(offset, its bytes)` in the order written.
    fn content_with_zeros(
        &mut self,
        view: &View,
        zeros: &mut [Zero],
        (text_prefix, declare): (&str, &str),
        (reuse, base): (&[&HashMap<String, String>], Option<&Base<'_>>),
        scope: u32,
    ) -> Result<(String, Vec<(usize, String)>), WriteError> {
        let total = view.chars.chars().count();
        if zero::arrange(zeros, total).is_some() {
            return Err(WriteError::Mismatch("a zero-width element is put past the end of its paragraph"));
        }
        let (_, texts) = self.zero_bytes(zeros, scope)?;
        let mut items: Vec<(usize, String)> = Vec::new();
        items.try_reserve_exact(zeros.len()).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
        // One stretch of characters for each run: the whole paragraph, with no span, when it has none.
        let whole = [runs::Run { start: 0, end: total, props: None }];
        let stretches: &[runs::Run] = if view.runs.is_empty() { &whole } else { &view.runs };
        let mut writer = zero::Spaces::new(&view.chars, text_prefix, declare);
        let mut out = String::new();
        let mut chars = view.chars.char_indices().peekable();
        let (mut pos, mut next) = (0usize, 0usize);
        self.work.order_steps = self.work.order_steps.saturating_add(zeros.len() as u64);
        let put = |writer: &mut zero::Spaces<'_>, out: &mut String, items: &mut Vec<(usize, String)>, k: usize| -> Result<(), WriteError> {
            let (Some(z), Some(text)) = (zeros.get(k), texts.get(k)) else {
                return Err(WriteError::Mismatch("a zero-width element is not there to put"));
            };
            if z.field.is_some() {
                writer.field(z.len);
            }
            out.push_str(text);
            items.push((z.at, text.clone()));
            Ok(())
        };
        for run in stretches {
            // Elements at the run's start that are no field's characters go before its span.
            while zeros.get(next).is_some_and(|z| z.at <= run.start && z.len == 0) {
                put(&mut writer, &mut out, &mut items, next)?;
                next += 1;
            }
            let style = match run.props.as_deref().filter(|p| p.writes()) {
                Some(props) => {
                    let style = self.run_style(props, reuse, base.map(|b| (b, run.start)))?;
                    self.work.charge_attribute(self.limits)?;
                    out.push_str(&format!(
                        "<{text_prefix}:span{declare} {text_prefix}:style-name=\"{}\">",
                        super::splice::escape_value(&style, b'"')
                    ));
                    true
                }
                None => false,
            };
            while let Some(z) = zeros.get(next).filter(|z| z.at < run.end) {
                let (at, len) = (z.at, z.len);
                writer.text(take(&view.chars, &mut chars, at.saturating_sub(pos))?, &mut out);
                pos = at;
                // A field's characters are what its element shows, and are not written again (a mark has none).
                if at.saturating_add(len) > run.end {
                    return Err(WriteError::Unsupported(WORDS_FIELD_CUT));
                }
                take(&view.chars, &mut chars, len)?;
                pos = pos.saturating_add(len);
                put(&mut writer, &mut out, &mut items, next)?;
                next += 1;
            }
            writer.text(take(&view.chars, &mut chars, run.end.saturating_sub(pos))?, &mut out);
            pos = run.end.max(pos);
            if style {
                out.push_str(&format!("</{text_prefix}:span>"));
            }
        }
        // What is left stands at the paragraph's end.
        while next < zeros.len() {
            put(&mut writer, &mut out, &mut items, next)?;
            next += 1;
        }
        Ok((out, items))
    }

    /// Replaces the content of a paragraph: between its tags, or, for one written empty, by opening it.
    fn put_content(&mut self, empty: bool, open: (usize, usize), content: (usize, usize), name: &str, text: String) {
        if empty {
            self.splicer.replace(open.1.saturating_sub(2), open.1, format!(">{text}</{name}>"));
        } else {
            self.splicer.replace(content.0, content.1, text);
        }
    }

    /// Puts the model's paragraph `new` into the text (see the module documentation), after the paragraph of the file `prev` (before the one `next` when there
    /// is none before it).
    ///
    /// **Beside a paragraph that has its row** it is put next to that paragraph's unit (a new item of the list when the neighbour is an item). **Where no
    /// neighbour has its row** it is put outside any list: after the list the paragraph before it is in, or before the list the one after it is in, and
    /// refused when it would fall between two items of one list (the specification does not say how a list is split). A paragraph with a list's row there is the
    /// item of a new list, and the paragraphs that come so in a row at one place are the items of one list (`pending`).
    ///
    /// **The paragraphs put in at one place are written in the model's order** (`cache.last`): one that has the row of a list item beside it would go inside the
    /// list, which can be before a paragraph already put in after the list, and is put after that paragraph and outside the list instead; where there is no
    /// such place (the paragraph already put in is a new item of a list, and this one would fall before it, between it and the next item) it is refused.
    fn insert_para(
        &mut self,
        paras: &[ParaAt],
        edit: &TextEdit,
        (new_ix, new, zeros): (usize, &View, &mut Vec<Zero>),
        (prev, next): (Option<usize>, Option<usize>),
        cache: &mut NearCache,
        pending: &mut Option<PendingList>,
    ) -> Result<(), WriteError> {
        let mut neighbour = [(prev, true), (next, false)]
            .into_iter()
            .filter_map(|(k, after)| k.map(|k| (k, after)))
            .find(|(k, _)| edit.old.get(*k).is_some_and(|o| o.pp_same(new)));
        if let (Some((k, after)), Some((last, outside))) = (neighbour, cache.last) {
            let at = self.para_at(paras, k)?;
            let (lead, end) = at.unit().unwrap_or((at.lead, at.end));
            if (if after {
                end
            } else if at.item.is_none() {
                at.before(lead)
            } else {
                lead
            }) < last
            {
                if !outside {
                    return Err(WriteError::Unsupported(WORDS_BETWEEN));
                }
                neighbour = None;
            }
        }
        let anchor = prev
            .map(|k| (k, true))
            .or(next.map(|k| (k, false)))
            .ok_or(WriteError::Mismatch("a paragraph is inserted where no paragraph of the file is"))?;
        let bullet = new.pp.as_deref().is_some_and(|p| p.bullet);
        let (style, wrapped, near, as_list) = match neighbour {
            Some((k, after)) => {
                let at = self.para_at(paras, k)?;
                // The neighbour's own style and its own place in a list, whatever its row is: they are what its row says.
                let name = match cache.names.get(&k) {
                    Some(found) => found.clone(),
                    None => {
                        let found = self.style_name_of(&at)?;
                        cache.names.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: k as u64 })?;
                        cache.names.insert(k, found.clone());
                        found
                    }
                };
                // The neighbour's style is named once more, by the paragraph put beside it.
                if let (Some(n), Some(book)) = (&name, self.book.as_mut())
                    && let Some(i) = book.find_def(n, Fam::Paragraph)
                {
                    book.add_reference(i);
                }
                let wrapped = match at.item {
                    None => false,
                    Some(item) if item.alone && !item.header => true,
                    Some(_) => return Err(WriteError::Unsupported(WORDS_ITEM)),
                };
                let (lead, end) = at.unit().unwrap_or((at.lead, at.end));
                (
                    name,
                    wrapped,
                    (
                        at,
                        if after {
                            end
                        } else if at.item.is_none() {
                            at.before(lead)
                        } else {
                            lead
                        },
                    ),
                    false,
                )
            }
            None => {
                // Between two items of one list: no place outside it.
                if let (Some(p), Some(n)) = (prev, next) {
                    let (before, after) = (self.para_at(paras, p)?.list, self.para_at(paras, n)?.list);
                    if before.is_some() && before == after {
                        return Err(WriteError::Unsupported(WORDS_BETWEEN));
                    }
                }
                let at = self.para_at(paras, anchor.0)?;
                let (lead, end) = at.top();
                (self.paragraph_style(&new.pp)?, false, (at, if anchor.1 { end } else { at.before(lead) }), bullet)
            }
        };
        let (at, place) = near;
        // Where the paragraph goes, and whether that is outside any list: the next one put in at this place goes no earlier than here.
        if cache.last.is_some_and(|(last, _)| place < last) {
            return Err(WriteError::Unsupported(WORDS_BETWEEN));
        }
        cache.last = Some((place, neighbour.is_none() || at.item.is_none()));
        cache
            .inserted
            .try_reserve(1)
            .map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: cache.inserted.len() as u64 })?;
        cache.inserted.push((new_ix, style.clone()));
        let pfx = Pfx::of(&mut self.scopes, at.scope, self.work);
        // The neighbours' spans name the styles of the runs they hold: a run of the same row in the new paragraph is given the same.
        let mut maps: Vec<Rc<HashMap<String, String>>> = Vec::new();
        for k in [prev, next].into_iter().flatten() {
            if let (Ok(near), Some(old)) = (self.para_at(paras, k), edit.old.get(k)) {
                maps.push(self.reuse_of(k, &near, old, cache)?);
            }
        }
        let refs: Vec<&HashMap<String, String>> = maps.iter().map(|m| &**m).collect();
        // The breaks that stood between paragraphs and land on this paragraph's start are written before it, or at the start of its list item (R10 item 5).
        let lead = self.take_between(zeros, at.scope)?;
        let body = if zeros.is_empty() {
            if cache.checking {
                cache.expect.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
                cache.expect.push(zero::Expect::Exact { index: new_ix, items: Vec::new() });
            }
            self.content_of(new, &pfx.text, "", &refs, (&HashMap::new(), None))?
        } else {
            let (body, items) = self.content_with_zeros(new, zeros, (&pfx.text, ""), (&refs, None), at.scope)?;
            cache.expect.try_reserve(1).map_err(|_| WriteError::Limit { what: zero::MEMORY_WORDS, max: 0 })?;
            cache.expect.push(zero::Expect::Exact { index: new_ix, items });
            body
        };
        self.work.charge_copy(new.chars.len(), self.limits)?;
        self.work.charge_attribute(self.limits)?;
        let t = &pfx.text;
        let attr = style.as_deref().map(|n| format!(" {t}:style-name=\"{}\"", super::splice::escape_value(n, b'"'))).unwrap_or_default();
        if as_list {
            // An item of a list this save makes: the start tag of the list carries the declarations, and the item and its paragraph none.
            let paragraph = if body.is_empty() { format!("<{t}:p{attr}/>") } else { format!("<{t}:p{attr}>{body}</{t}:p>") };
            let item = format!("<{t}:list-item>{lead}{paragraph}</{t}:list-item>");
            let props = new.pp.clone().ok_or(WriteError::Mismatch("a list item with no row"))?;
            match pending {
                Some(list) if list.place == place && list.props.key == props.key => {
                    list.items
                        .try_reserve(1)
                        .map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: list.items.len() as u64 })?;
                    list.items.push(item);
                }
                _ => {
                    self.flush_list(pending)?;
                    *pending = Some(PendingList { place, props, text: pfx.text.clone(), declare: pfx.declare.clone(), items: vec![item] });
                }
            }
            return Ok(());
        }
        let (declare_on_p, declare_on_item) = if wrapped { (String::new(), pfx.declare.clone()) } else { (pfx.declare.clone(), String::new()) };
        let paragraph = if body.is_empty() { format!("<{t}:p{declare_on_p}{attr}/>") } else { format!("<{t}:p{declare_on_p}{attr}>{body}</{t}:p>") };
        let element =
            if wrapped { format!("<{t}:list-item{declare_on_item}>{lead}{paragraph}</{t}:list-item>") } else { format!("{lead}{paragraph}") };
        self.flush_list(pending)?;
        self.splicer.insert(place, element);
        Ok(())
    }

    /// The styles the spans of the file's paragraph at `at` (which the baseline has as `old`) name for its runs, by the row of each run.
    /// **Found once for each paragraph** (`cache`): the paragraph's content is tokenized, charged, the first time a paragraph is put beside it, and never again.
    fn reuse_of(&mut self, ix: usize, at: &ParaAt, old: &View, cache: &mut NearCache) -> Result<Rc<HashMap<String, String>>, WriteError> {
        if let Some(found) = cache.reuse.get(&ix) {
            return Ok(Rc::clone(found));
        }
        let mut reuse: HashMap<String, String> = HashMap::new();
        if old.runs.is_empty() {
            let none = Rc::new(reuse);
            cache.reuse.insert(ix, Rc::clone(&none));
            return Ok(none);
        }
        let pfx = Pfx::of(&mut self.scopes, at.scope, self.work);
        let content = if at.empty { (at.open.1, at.open.1) } else { (at.open.1, at.close_start) };
        let ns_of = |n: &str| -> Option<&'static str> {
            let (prefix, local) = n.split_once(':')?;
            (prefix == pfx.text).then(|| local_name(local))
        };
        self.work.charge_tokens(content.1.saturating_sub(content.0), self.limits)?;
        if let Some((tokens, _)) = tokenize(self.bytes, content.0, content.1, &ns_of) {
            collect_reuse(&tokens, &old.runs, &mut reuse, &mut self.work.order_steps)?;
        }
        let found = Rc::new(reuse);
        cache.reuse.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the paragraphs of a shape", max: ix as u64 })?;
        cache.reuse.insert(ix, Rc::clone(&found));
        Ok(found)
    }

    /// The `text:style-name` the paragraph at `at` has, when it names a style (read from its start tag: asked once for each paragraph, `NearCache`).
    fn style_name_of(&mut self, at: &ParaAt) -> Result<Option<String>, WriteError> {
        let tag = TagEdit::untracked(self.bytes, at.open, at.scope, self.work)?;
        Ok(tag.find(&mut self.scopes, Ns::Text, "style-name", self.work).map(|a| tag.text(a)))
    }

    /// Sets, takes out or adds the `text:style-name` of the element whose start tag is `open`, keeping the count of what names each style.
    fn set_style_name(&mut self, open: (usize, usize), scope: u32, name: Option<&str>, family: Fam) -> Result<(), WriteError> {
        let mut tag = TagEdit::untracked(self.bytes, open, scope, self.work)?;
        let at = tag.find(&mut self.scopes, Ns::Text, "style-name", self.work);
        let current = at.map(|a| tag.text(a));
        if current.as_deref() == name {
            return Ok(());
        }
        self.work.charge_attribute(self.limits)?;
        match (at, name) {
            (Some(a), Some(n)) => tag.set(&mut self.splicer, a, n)?,
            (Some(a), None) => tag.remove(&mut self.splicer, a)?,
            (None, Some(n)) => tag.add(&mut self.splicer, &mut self.scopes, self.work, Ns::Text, "style-name", n)?,
            (None, None) => {}
        }
        if let Some(book) = self.book.as_mut() {
            if let Some(i) = current.as_deref().and_then(|c| book.find_def(c, family)) {
                book.drop_reference(i);
            }
            if let Some(i) = name.and_then(|n| book.find_def(n, family)) {
                book.add_reference(i);
            }
        }
        Ok(())
    }

    /// The automatic paragraph style that holds the attributes of the row `pp`: `None` for a row that writes none (the paragraph then names no style). The cells
    /// of a list's row that are the list's (the bullet, the margin, the indent) are in the list style ([`Editor::list_style`]), and the others are the style's.
    fn paragraph_style(&mut self, pp: &Option<Arc<Props>>) -> Result<Option<String>, WriteError> {
        match pp.as_deref() {
            Some(p) => self.style_for(Fam::Paragraph, p),
            None => Ok(None),
        }
    }

    /// The style of a run: the one a span of the paragraph named for a run of the same row (`reuse`), when the part still has it, else the one that holds the
    /// attributes of the row.
    ///
    /// **A run whose row changed** (`base`: the old paragraph and the place in the new one the run starts at) takes the style of the run it stands in the place of, with
    /// the attributes of the cells that changed written again and every other attribute as the file has it ([`Editor::derived_style`]); a run that stands in the
    /// place of none, or of a run with no style, is a style of the attributes of its row.
    fn run_style(&mut self, props: &Props, reuse: &[&HashMap<String, String>], base: Option<(&Base<'_>, usize)>) -> Result<String, WriteError> {
        if let Some(name) = reuse.iter().find_map(|m| m.get(&props.key))
            && let Some(book) = self.book.as_mut()
            && let Some(i) = book.find_def(name, Fam::Text)
        {
            book.add_reference(i);
            return Ok(name.clone());
        }
        if let Some((base, start)) = base {
            self.work.order_steps = self.work.order_steps.saturating_add(1);
            if let Some((name, old)) = base.run_at(start)
                && let Some(made) = self.derived_style(Fam::Text, name, old, props, base.in_place.contains(name))?
            {
                return Ok(made);
            }
        }
        self.text_style(props)
    }

    /// The style of a paragraph written whole whose row changed, made from the style the file's paragraph names: the attributes of the cells that changed written
    /// again, every other attribute and element as the file has it (the tab stops' leaders and the spelling of `start` and `end`, which the cells
    /// do not say, stay while their cells are unchanged). `None` when it cannot be made so (the paragraph names no automatic style of the part, or one of the rows
    /// is a list's, whose cells are the list's), and the style is then the row's attributes.
    fn derived_paragraph_style(&mut self, at: &ParaAt, old: &View, new: &View, may_stand: bool) -> Result<Option<String>, WriteError> {
        let Some(new_pp) = new.pp.as_deref().filter(|p| !p.bullet) else { return Ok(None) };
        if old.pp.as_deref().is_some_and(|p| p.bullet) {
            return Ok(None);
        }
        let Some(base) = self.style_name_of(at)? else { return Ok(None) };
        let nothing = Props { key: String::new(), ops: Vec::new(), faces: Vec::new(), bullet: false, list: None, parts: Vec::new() };
        let old_pp = old.pp.as_deref().unwrap_or(&nothing);
        // A paragraph style is edited where it stands when `may_stand` and nothing else names it (the paragraph's own `text:style-name` is its one reference): the
        // count of references is looked at once, where the style is made ([`Editor::derived_style`]).
        self.derived_style(Fam::Paragraph, &base, old_pp, new_pp, may_stand)
    }

    /// The styles of the file that the paragraph written whole may edit where they stand (a style is cloned only when another paragraph or run names it, and
    /// no style is renamed): the style a **single** new run takes (derived from the one run of the file that names it, by one pair of rows) and that no other run of the
    /// paragraph uses as it is (a run whose row is unchanged takes the style the file's span has for it, which an edit in place would have changed under it). That nothing
    /// else names the style (one reference, the span being rewritten) is looked at where the style is made ([`Editor::derived_style`]).
    fn in_place_styles(
        &mut self,
        new: &View,
        own: &HashMap<String, String>,
        base: &Base<'_>,
    ) -> Result<std::collections::HashSet<String>, WriteError> {
        let mut out = std::collections::HashSet::new();
        if new.runs.is_empty() {
            return Ok(out);
        }
        let held = (new.runs.len() as u64).saturating_mul(RUN_STYLE_BYTES);
        self.work.hold(held, MEMORY_WORDS)?;
        let found = self.in_place_needs(new, own, base, &mut out);
        self.work.release(held);
        found.map(|()| out)
    }

    /// [`Editor::in_place_styles`], with the room it needs held: the style each run of the new paragraph needs, one step for each run.
    fn in_place_needs(
        &mut self,
        new: &View,
        own: &HashMap<String, String>,
        base: &Base<'_>,
        out: &mut std::collections::HashSet<String>,
    ) -> Result<(), WriteError> {
        // For each style of the file a run of the paragraph needs: whether a run uses it as it is, and the pairs of rows (the file's run, the new run) that derive it.
        type Need<'n> = (bool, std::collections::HashSet<(&'n str, &'n str)>);
        let mut needs: HashMap<&str, Need<'_>> = HashMap::new();
        needs
            .try_reserve(new.runs.len().min(MOST_NEEDS_RESERVED))
            .map_err(|_| WriteError::Limit { what: MEMORY_WORDS, max: new.runs.len() as u64 })?;
        for run in &new.runs {
            self.work.order_steps = self.work.order_steps.saturating_add(1);
            let Some(props) = run.props.as_deref().filter(|p| p.writes()) else { continue };
            let reused = own.get(&props.key).filter(|name| self.book.as_ref().is_some_and(|b| b.find_def(name, Fam::Text).is_some()));
            if let Some(name) = reused {
                needs.entry(name.as_str()).or_default().0 = true;
            } else if let Some((name, old)) = base.run_at(run.start) {
                needs.entry(name).or_default().1.insert((old.key.as_str(), props.key.as_str()));
            }
        }
        for (name, (reused, derived)) in needs {
            if !reused && derived.len() == 1 {
                out.try_reserve(1).map_err(|_| WriteError::Limit { what: MEMORY_WORDS, max: out.len() as u64 })?;
                out.insert(name.to_owned());
            }
        }
        Ok(())
    }

    /// The automatic style of `family` that is the style `base` of the part with the cells of the row `old` changed into those of `new` (each value is
    /// regenerated by the cell it comes from and is kept while that cell is unchanged, and an attribute no cell maps stays): the style itself with [`runs::changes`]
    /// made in its properties **where it stands** when `where_it_stands` allows it (the plan says no other run of the paragraph or paragraph put in wants what the style
    /// said) **and nothing else names the style** (one reference, the one being rewritten: counted here, once, where it is decided), else a copy of it under a new
    /// name (no style is renamed), put after it. The style as it is when nothing changes. Made once for a style and two rows. `None` when `base` is not an automatic style of this part.
    ///
    /// **A copy holds the whole style**, so what it adds to its part is charged before it is made ([`WriteWork::charge_tree_items`]: the items of the style, once
    /// counted, and the attributes written) against what the reader reads of one part: a retext that gives each of thousands of runs of one style a row of its own
    /// is refused in words and not saved as a file that cannot be opened again.
    fn derived_style(&mut self, family: Fam, base: &str, old: &Props, new: &Props, where_it_stands: bool) -> Result<Option<String>, WriteError> {
        let key = (family, base.to_owned(), old.key.clone(), new.key.clone());
        let named = match self.texts.derived.get(&key) {
            Some(done) => done.clone(),
            None => {
                let Some((at, def)) = self.book.as_ref().and_then(|b| b.find_def(base, family).and_then(|i| b.defs.get(i).map(|d| (i, d.clone()))))
                else {
                    return Ok(None);
                };
                let (mut ops, faces) = runs::changes(old, new);
                for &(local, ref family_name) in &faces {
                    let wanted = family_name.trim().trim_matches(|c| c == '\'' || c == '"').trim();
                    let face = self.face_name(wanted)?;
                    ops.push(Op { prop: 2, ns: Ns::Style, local, val: Val::Text(face) });
                }
                let prop = if family == Fam::Text { 2 } else { 1 };
                let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
                let alone = where_it_stands && self.book.as_ref().is_some_and(|b| b.live_refs(at) <= 1);
                let name = if alone {
                    // Nothing else names the style: it is edited where it stands, and the index of styles by their attributes leaves it out (it no longer says what the file's
                    // bytes do): a style in `edited` is a miss when the index is asked ([`Editor::existing_style`]), and the index itself is not walked (a walk of every key
                    // for each edit in place was quadratic in the edits).
                    // The edit adds at most an attribute for each attribute it writes and the element when the style has none: charged before it is made, and given back
                    // when it changes nothing.
                    let items = (ops.len() as u64).saturating_add(2);
                    self.work.charge_tree_items(items, self.limits)?;
                    if self.edit_text_style_in_place(&def, prop, &ops, &mut wrote)? {
                        self.texts.edited.insert(at);
                        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?;
                        book.written.push(Written { name: base.to_owned(), family, attrs: wrote, prop });
                    } else {
                        self.work.refund_tree_items(items);
                    }
                    base.to_owned()
                } else {
                    let items = self.style_items(&def)?.saturating_add(ops.len() as u64);
                    self.work.charge_tree_items(items, self.limits)?;
                    // A copy is made only when the edit changes something of the style, and its name is the next free one only then.
                    match self.clone_text_style(&def, prop, &ops, &mut wrote)? {
                        None => {
                            self.work.refund_tree_items(items);
                            base.to_owned()
                        }
                        Some(copy) => {
                            let name = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?.names.next(family);
                            let source = self.bytes.get(def.lead..def.end).ok_or(WriteError::Mismatch("a style that is not in the part"))?;
                            let copy = super::styles::rename_copy(&copy, source, &def, &name)?;
                            self.work.charge_style_copy(copy.len(), self.limits)?;
                            self.splicer.insert(def.end, copy);
                            let book = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?;
                            book.written.push(Written { name: name.clone(), family, attrs: wrote, prop });
                            name
                        }
                    }
                };
                // The keys the save holds are charged as the copies are: a row's key is its cells, and a hostile model has many rows.
                self.work
                    .charge_style_key(key.1.len().saturating_add(key.2.len()).saturating_add(key.3.len()).saturating_add(name.len()), self.limits)?;
                self.texts.derived.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
                self.texts.derived.insert(key, name.clone());
                name
            }
        };
        // The style of the file that stays is named once more; a copy is the save's own and is counted by nothing.
        if let Some(book) = self.book.as_mut()
            && let Some(i) = book.find_def(&named, family)
        {
            book.add_reference(i);
        }
        Ok(Some(named))
    }

    /// Writes the list the save has been collecting items for, if there is one: a `text:list` of the automatic list style the row of its items asks for.
    fn flush_list(&mut self, pending: &mut Option<PendingList>) -> Result<(), WriteError> {
        let Some(list) = pending.take() else { return Ok(()) };
        let name = self.list_style(&list.props)?;
        self.work.charge_attribute(self.limits)?;
        let t = &list.text;
        let text =
            format!("<{t}:list{} {t}:style-name=\"{}\">{}</{t}:list>", list.declare, super::splice::escape_value(&name, b'"'), list.items.concat());
        self.splicer.insert(list.place, text);
        Ok(())
    }

    /// The automatic list style of one level that holds the bullet a list's row asks for (the table's character or `BulletStr`, the face and the size of
    /// the bullet, and the label's margin and indent), made once for each row and named by R27 (`L` and a number when the file has no list style).
    fn list_style(&mut self, props: &Props) -> Result<String, WriteError> {
        if let Some(done) = self.texts.lists.get(&props.key) {
            return Ok(done.clone());
        }
        let spec = props.list.as_ref().ok_or(WriteError::Mismatch("a list made from a row that has no bullet"))?;
        // A list level's bullet is one character; LibreOffice keeps the first of a longer string, which is no bullet the model has.
        if spec.bullet.chars().count() != 1 {
            return Err(WriteError::Unsupported(WORDS_BULLET_STR));
        }
        let font = match &spec.font {
            Some(family) => Some(self.face_name(family.trim().trim_matches(|c| c == '\'' || c == '"').trim())?),
            None => None,
        };
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a list style without the styles of the part"))?;
        let name = book.names.next(Fam::List);
        let scope =
            book.auto.as_ref().map(|a| a.scope).ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let (t, st, fo) = (&pfx.text, &pfx.style, &pfx.fo);
        let unit = self.unit;
        let length = |inches: f64| length_text(inches, unit).ok_or(WriteError::Unsupported("a length too large to write"));
        // Every attribute written is charged before the element is made.
        let mut attributes = 3u64;
        let mut align = String::new();
        if let Some(x) = spec.ind_first {
            align.push_str(&format!(" {fo}:text-indent=\"{}\"", length(x)?));
            attributes += 1;
        }
        if let Some(x) = spec.ind_left {
            align.push_str(&format!(" {fo}:margin-left=\"{}\"", length(x)?));
            attributes += 1;
        }
        let level = if align.is_empty() {
            String::new()
        } else {
            attributes += 3;
            format!(
                "<{st}:list-level-properties {t}:list-level-position-and-space-mode=\"label-alignment\"><{st}:list-level-label-alignment {t}:label-followed-by=\"listtab\"{align}/></{st}:list-level-properties>"
            )
        };
        let mut face = String::new();
        if let Some(f) = &font {
            face.push_str(&format!(" {st}:font-name=\"{}\"", super::splice::escape_value(f, b'"')));
            attributes += 1;
        }
        if let Some(x) = spec.size {
            face.push_str(&format!(" {fo}:font-size=\"{}\"", length(x)?));
            attributes += 1;
        }
        let text_props = if face.is_empty() { String::new() } else { format!("<{st}:text-properties{face}/>") };
        for _ in 0..attributes {
            self.work.charge_attribute(self.limits)?;
        }
        self.work.charge_copy(spec.bullet.len(), self.limits)?;
        let text = format!(
            "<{t}:list-style{} {st}:name=\"{}\"><{t}:list-level-style-bullet {t}:level=\"1\" {t}:bullet-char=\"{}\">{level}{text_props}</{t}:list-level-style-bullet></{t}:list-style>",
            pfx.declare,
            super::splice::escape_value(&name, b'"'),
            super::splice::escape_value(&spec.bullet, b'"')
        );
        self.put_style(text)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a list style without the styles of the part"))?;
        book.written.push(Written { name: name.clone(), family: Fam::List, attrs: Vec::new(), prop: 0 });
        self.texts.lists.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
        self.texts.lists.insert(props.key.clone(), name.clone());
        Ok(name)
    }

    /// The name of the `style:font-face` of this part for `family`: the one it declares, else one this save adds (a face is written through a declaration in
    /// the same file's `office:font-face-decls`, added when missing).
    fn face_name(&mut self, family: &str) -> Result<String, WriteError> {
        if self.texts.by_family.is_none() {
            let mut map: HashMap<String, String> = HashMap::new();
            map.try_reserve(self.texts.faces.len())
                .map_err(|_| WriteError::Limit { what: "memory for the font faces of a part", max: self.texts.faces.len() as u64 })?;
            for face in &self.texts.faces {
                map.entry(face.family.clone()).or_insert_with(|| face.name.clone());
            }
            self.work.order_steps = self.work.order_steps.saturating_add(self.texts.faces.len() as u64);
            self.texts.by_family = Some(map);
        }
        if let Some(name) = self.texts.by_family.as_ref().and_then(|m| m.get(family)) {
            return Ok(name.clone());
        }
        self.add_face(family)
    }

    /// The names of the part's faces, and those this save adds, as a set: made once, the first time a name is wanted.
    fn index_face_names(&mut self) -> Result<(), WriteError> {
        if self.texts.face_names.is_none() {
            let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
            names
                .try_reserve(self.texts.faces.len())
                .map_err(|_| WriteError::Limit { what: "memory for the font faces of a part", max: self.texts.faces.len() as u64 })?;
            names.extend(self.texts.faces.iter().map(|f| f.name.clone()));
            self.work.order_steps = self.work.order_steps.saturating_add(self.texts.faces.len() as u64);
            self.texts.face_names = Some(names);
        }
        Ok(())
    }

    /// The name of a declared face for a name a held style gives a face, `local` being the attribute that gives it (`font-name`, `font-name-asian` or `font-name-complex`): **the name itself while
    /// the part declares a face of it**, and when it does not, the declared face of the family the cells put the paragraph's own text in for that script, which a declaration is added for when the
    /// part has none (GATE1622-CROSS-FONTNAME: a table that lost a declaration wrote the name of it, which no face answers to, and LibreOffice drew the text in its fallback face). A name no cell of
    /// the paragraph stands for is the file's own and is written as it was. The look is one step.
    pub(crate) fn declared_face_of(&mut self, local: &str, name: &str) -> Result<String, WriteError> {
        if self.texts.own_faces.is_empty() {
            return Ok(name.to_owned());
        }
        self.index_face_names()?;
        if self.texts.face_names.as_ref().is_some_and(|names| names.contains(name)) {
            return Ok(name.to_owned());
        }
        let Some(family) =
            self.texts.own_faces.iter().find(|(l, _)| *l == local).map(|(_, f)| f.trim().trim_matches(|c| c == '\'' || c == '"').trim().to_owned())
        else {
            return Ok(name.to_owned());
        };
        if family.is_empty() {
            return Ok(name.to_owned());
        }
        self.face_name(&family)
    }

    /// Adds the declaration of a face of `family`, named by the family itself (and a number when a face of another family has the name).
    fn add_face(&mut self, family: &str) -> Result<String, WriteError> {
        if family.is_empty() || super::splice::has_bad_char(family) || family.contains('\r') {
            return Err(WriteError::Unsupported(WORDS_FACE));
        }
        let auto = self.book.as_ref().and_then(|b| b.auto.clone()).filter(|a| !a.made);
        let Some(auto) = auto else {
            return Err(WriteError::Unsupported("a part with no office:automatic-styles to put a font declaration before"));
        };
        let scope = self.fonts.as_ref().map_or(auto.scope, |f| f.scope);
        // The name: the family, or the family and a number while a face of the part has the name. The names of the faces are indexed once (a look through all of
        // them for each number tried is quadratic in the faces), and each number tried is a step, charged before the next is made.
        self.index_face_names()?;
        let mut name = family.to_owned();
        let mut number = 1u64;
        while self.texts.face_names.as_ref().is_some_and(|names| names.contains(&name)) {
            if !self.work.step() {
                self.work.check_steps()?;
            }
            number = number.saturating_add(1);
            name = format!("{family} {number}");
        }
        if let Some(names) = self.texts.face_names.as_mut() {
            names.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the font faces of a part", max: 0 })?;
            names.insert(name.clone());
        }
        for _ in 0..2 {
            self.work.charge_attribute(self.limits)?;
        }
        self.work.charge_copy(family.len().saturating_mul(2), self.limits)?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        // The family between single quotes, or between double ones (written as references: the attribute's own quotes are double) when it holds a single quote.
        let quoted = if family.contains('\'') {
            format!("&quot;{}&quot;", super::splice::escape_value(family, b'"'))
        } else {
            format!("'{}'", super::splice::escape_value(family, b'"'))
        };
        let (st, svg) = (&pfx.style, &pfx.svg);
        self.texts.new_faces.push_str(&format!(
            "<{st}:font-face{} {st}:name=\"{}\" {svg}:font-family=\"{quoted}\"/>",
            pfx.declare,
            super::splice::escape_value(&name, b'"')
        ));
        self.texts.faces_added.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the font faces of a part", max: 0 })?;
        self.texts.faces_added.push((name.clone(), family.to_owned()));
        self.texts.faces.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the font faces of a part", max: 0 })?;
        self.texts.faces.push(super::scan::FaceDef { name: name.clone(), family: family.to_owned() });
        if let Some(map) = self.texts.by_family.as_mut() {
            map.entry(family.to_owned()).or_insert_with(|| name.clone());
        }
        Ok(name)
    }

    /// Charges to the steps of the save the keys the walks of the indexes of this part's styles went over ([`StyleKeys::retain`]): a walk of an index is paid for where
    /// the save is counted, whoever walks it.
    pub(crate) fn count_index_walks(&mut self) {
        let walked = self.texts.index_walks();
        self.work.order_steps = self.work.order_steps.saturating_add(walked);
    }

    /// Puts the declarations of the faces this save added into `office:font-face-decls`, after the last, or makes the element (before `office:automatic-styles`)
    /// when the part has none.
    pub(crate) fn finish_faces(&mut self) {
        if self.texts.new_faces.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.texts.new_faces);
        match self.fonts.clone() {
            Some(f) if f.empty => self.splicer.replace(f.open.1.saturating_sub(2), f.open.1, format!(">{text}</{}>", f.qname)),
            Some(f) => self.splicer.insert(f.close_start, text),
            None => {
                let Some(auto) = self.book.as_ref().and_then(|b| b.auto.clone()) else { return };
                let qname = auto.qname.replacen("automatic-styles", "font-face-decls", 1);
                match self.fonts_at {
                    // A flat drawing: at its place in the order, before the `office:automatic-styles` a save makes at the same place.
                    Some(at) => self.splicer.insert_first(at, format!("<{qname}>{text}</{qname}>")),
                    None => self.splicer.insert(auto.open.0, format!("<{qname}>{text}</{qname}>")),
                }
            }
        }
    }

    /// The automatic text style that holds the attributes of a run's row.
    fn text_style(&mut self, props: &Props) -> Result<String, WriteError> {
        self.style_for(Fam::Text, props)?.ok_or(WriteError::Mismatch("a run with attributes has no style"))
    }

    /// The attributes of `props` as ops, the faces it names resolved to the declarations of the part.
    fn style_ops(&mut self, props: &Props) -> Result<Vec<Op>, WriteError> {
        self.style_ops_without(props, &[])
    }

    /// [`Editor::style_ops`] but for the attributes of the groups in `skip` (the group names [`super::runs::owned`] knows): their ops are not made and **their faces are not declared**.
    pub(crate) fn style_ops_without(&mut self, props: &Props, skip: &[&str]) -> Result<Vec<Op>, WriteError> {
        let owned = |ns: Ns, local: &str| skip.iter().any(|id| super::runs::owned(id).contains(&(ns, local)));
        let mut ops: Vec<Op> = props.ops.iter().filter(|o| !owned(o.ns, o.local)).cloned().collect();
        if props.faces.is_empty() {
            return Ok(ops);
        }
        for (local, family) in props.faces.iter().filter(|(local, _)| !owned(Ns::Style, local)) {
            let wanted = family.trim().trim_matches(|c| c == '\'' || c == '"').trim();
            let name = self.face_name(wanted)?;
            ops.push(Op { prop: 2, ns: Ns::Style, local, val: Val::Text(name) });
        }
        Ok(ops)
    }

    /// The canonical text of a set of attributes, as the part would hold them: what two styles with one set of attributes have in common.
    fn ops_key(&self, ops: &[Op]) -> Result<String, WriteError> {
        let mut parts: Vec<String> = Vec::new();
        for o in ops {
            let value = match &o.val {
                Val::Text(t) => t.clone(),
                Val::Length(inches) => length_text(*inches, self.unit).ok_or(WriteError::Unsupported("a length too large to write"))?,
                Val::Remove => continue,
            };
            parts.push(format!("{:?}:{}={}", o.ns, o.local, value));
        }
        parts.sort_unstable();
        Ok(parts.join(";"))
    }

    /// The style of `family` that holds the attributes of `props`: the one this save made or found for them, one the part holds with just these attributes (a
    /// style of no parent whose only property element is of the family's kind), or a new one named by R27.
    fn style_for(&mut self, family: Fam, props: &Props) -> Result<Option<String>, WriteError> {
        if let Some(done) = self.texts.made.get(&family).and_then(|m| m.get(&props.key)) {
            return Ok(done.clone());
        }
        let ops = self.style_ops(props)?;
        let made = if ops.is_empty() {
            None
        } else {
            let key = self.ops_key(&ops)?;
            let found = self.existing_style(family, &key)?;
            Some(match found {
                Some(name) => name,
                None => self.make_style(family, &ops)?,
            })
        };
        let remembered = self.texts.made.entry(family).or_default();
        remembered.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: 0 })?;
        remembered.insert(props.key.clone(), made.clone());
        Ok(made)
    }

    /// The name of a style the part holds with exactly the attributes `key` says, counted as named once more.
    fn existing_style(&mut self, family: Fam, key: &str) -> Result<Option<String>, WriteError> {
        let prop = if family == Fam::Text { 2 } else { 1 };
        if !self.texts.index.contains_key(&family) {
            let defs: Vec<(usize, StyleDef)> =
                self.book.as_ref().map(|b| b.defs.iter().cloned().enumerate().filter(|(_, d)| d.family == family).collect()).unwrap_or_default();
            let mut index: HashMap<String, usize> = HashMap::new();
            index.try_reserve(defs.len()).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: defs.len() as u64 })?;
            for (i, def) in &defs {
                self.work.order_steps = self.work.order_steps.saturating_add(1);
                // A style this save edited where it stands no longer says what the file's bytes say of it.
                if self.texts.edited.contains(i) {
                    continue;
                }
                if let Some(k) = self.style_key(def, prop)? {
                    index.entry(k).or_insert(*i);
                }
            }
            self.texts.index.insert(family, StyleKeys::new(index));
        }
        // A style edited where it stands **after** the index was made is a miss as it is when the index is made later (above): what the index says of it is what the file
        // said, and the style says something else now. Looked at here, one look for a key, and not the index walked at each edit.
        let Some(i) = self.texts.index.get(&family).and_then(|m| m.get(key)).copied().filter(|i| !self.texts.edited.contains(i)) else {
            return Ok(None);
        };
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?;
        let name = book.defs.get(i).map(|d| d.name.clone());
        book.add_reference(i);
        Ok(name)
    }

    /// The canonical attributes of a style of the part that holds exactly one properties element of kind `prop` (1 paragraph, 2 text), written empty, and
    /// names no parent and no other attribute but its name and family: the style a row's attributes can stand for. `None` for any other.
    fn style_key(&mut self, def: &StyleDef, prop: usize) -> Result<Option<String>, WriteError> {
        let Some(props) = def.props.get(prop).copied().flatten() else { return Ok(None) };
        if !props.empty || def.props.iter().enumerate().any(|(k, p)| k != prop && p.is_some()) {
            return Ok(None);
        }
        let bytes = self.bytes;
        let blank = |from: usize, to: usize| bytes.get(from..to).is_some_and(diacraft_container::xml_scan::is_blank);
        if !blank(def.open.1, props.open.0) || !blank(props.end, def.close_start) {
            return Ok(None);
        }
        let mut spans: Vec<AttrSpan> = Vec::new();
        self.work.charge_attribute_bytes(def.open.1.saturating_sub(def.open.0))?;
        tag_attributes(bytes, def.open.0, def.open.1, &mut spans).ok_or(WriteError::Mismatch("a style tag the writer cannot read"))?;
        for a in &spans {
            let name = std::str::from_utf8(bytes.get(a.name.0..a.name.1).unwrap_or_default()).unwrap_or_default();
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let Some((prefix, local)) = name.split_once(':') else { return Ok(None) };
            let ns = self.scopes.ns_of_prefix(def.scope, prefix, self.work);
            if !(ns == Ns::Style && matches!(local, "name" | "family")) {
                return Ok(None);
            }
        }
        spans.clear();
        self.work.charge_attribute_bytes(props.open.1.saturating_sub(props.open.0))?;
        tag_attributes(bytes, props.open.0, props.open.1, &mut spans).ok_or(WriteError::Mismatch("a style tag the writer cannot read"))?;
        let mut parts: Vec<String> = Vec::new();
        for a in &spans {
            let name = std::str::from_utf8(bytes.get(a.name.0..a.name.1).unwrap_or_default()).unwrap_or_default();
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let Some((prefix, local)) = name.split_once(':') else { return Ok(None) };
            let ns = self.scopes.ns_of_prefix(props.scope, prefix, self.work);
            let value = crate::package::unescape(bytes.get(a.value.0..a.value.1).unwrap_or_default());
            parts.try_reserve(1).map_err(|_| WriteError::Limit { what: "memory for the styles of a part", max: parts.len() as u64 })?;
            parts.push(format!("{ns:?}:{local}={value}"));
        }
        parts.sort_unstable();
        Ok(Some(parts.join(";")))
    }

    /// A new automatic style of `family` with the attributes `ops`, put at the end of `office:automatic-styles`.
    fn make_style(&mut self, family: Fam, ops: &[Op]) -> Result<String, WriteError> {
        let (word, element, prop) = if family == Fam::Text { ("text", "text-properties", 2) } else { ("paragraph", "paragraph-properties", 1) };
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?;
        let name = book.names.next(family);
        let scope =
            book.auto.as_ref().map(|a| a.scope).ok_or(WriteError::Unsupported("a part with no office:automatic-styles to put a new style in"))?;
        let pfx = Pfx::of(&mut self.scopes, scope, self.work);
        let inner = pfx.without_declarations();
        let mut wrote: Vec<(Ns, &'static str)> = Vec::new();
        let attrs = self.ctx().attributes(&inner, ops, &mut wrote)?;
        let s = &pfx.style;
        let text = format!(
            "<{s}:style {s}:name=\"{}\" {s}:family=\"{word}\"{}><{s}:{element}{attrs}/></{s}:style>",
            super::splice::escape_value(&name, b'"'),
            pfx.declare
        );
        self.put_style(text)?;
        let book = self.book.as_mut().ok_or(WriteError::Mismatch("a retext without the styles of the part"))?;
        book.written.push(Written { name: name.clone(), family, attrs: wrote, prop });
        Ok(name)
    }
}

/// A `text:span` of the file whose run is unchanged, to be kept as it is: the bytes of the element, and the style it names.
#[derive(Clone, Debug)]
struct KeptSpan {
    bytes: (usize, usize),
    style: Option<String>,
}

/// The top-level spans of a paragraph that hold no other span, each as `(first character, end, the element's bytes, the style it names)`; `None` when the
/// characters the tokens stand for, as they are written, are not `total` (a paragraph whose white space the reader collapses is not matched with its runs).
fn plain_spans(tokens: &[Token], total: usize) -> Option<Vec<(usize, usize, KeptSpan)>> {
    let mut out: Vec<(usize, usize, KeptSpan)> = Vec::new();
    let (mut at, mut depth, mut dirty) = (0usize, 0usize, false);
    let mut open: Option<(usize, usize, Option<String>)> = None;
    // The span's first thing is still to come, and whether the last thing in it ends in white space: a span whose bytes begin or end with white space reads as
    // the characters it had only beside the characters it had (a space after another collapses), so it is not kept as its bytes where the text round it changed.
    let (mut first, mut ends_in_space) = (false, false);
    let space = |c: Option<&char>| c.is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r'));
    for t in tokens {
        match t {
            Token::SpanOpen { style, at: start } => {
                if depth == 0 {
                    open = Some((at, *start, style.clone()));
                    (dirty, first, ends_in_space) = (false, true, false);
                } else {
                    dirty = true;
                }
                depth += 1;
            }
            Token::SpanClose { end } => {
                depth = depth.saturating_sub(1);
                if depth == 0
                    && !dirty
                    && !ends_in_space
                    && let Some((first, start, style)) = open.take()
                {
                    out.try_reserve(1).ok()?;
                    out.push((first, at, KeptSpan { bytes: (start, *end), style }));
                }
            }
            Token::Text { chars, .. } => {
                if first && space(chars.first()) {
                    dirty = true;
                }
                (first, ends_in_space) = (false, space(chars.last()));
                at = at.saturating_add(chars.len());
            }
            Token::Element(space) => {
                (first, ends_in_space) = (false, false);
                at = at.saturating_add(space.len());
            }
            // A span that holds a zero-width element or a field is not kept as its bytes: the element is put by the offset it maps to, wherever the span is.
            Token::Mark { .. } | Token::Field { .. } => {
                if let Token::Field { chars, .. } = t {
                    at = at.saturating_add(chars.len());
                }
                dirty = true;
            }
        }
    }
    (at == total).then_some(out)
}

/// What the spans of a paragraph say of the styles of its runs: for each run of the baseline (`runs`), when the whole of it lies in one span that names a style and
/// sits in no other span, the style (`None` for any other run, and for all of them when the characters of the tokens, as they are written, are not the runs'
/// end: a paragraph whose white space the reader collapses is left alone).
fn run_style_names(tokens: &[Token], runs: &[runs::Run], steps: &mut u64) -> Result<Vec<Option<String>>, WriteError> {
    let mut out: Vec<Option<String>> = Vec::new();
    let Some(total) = runs.last().map(|r| r.end) else { return Ok(out) };
    // Memory that is not had is a refusal in words, and never a paragraph left with no style for its runs.
    out.try_reserve_exact(runs.len()).map_err(|_| WriteError::Limit { what: MEMORY_WORDS, max: runs.len() as u64 })?;
    // The segments of characters, each with the style of the span it is in (`None` outside any, and for one inside two).
    let mut segments: Vec<(usize, usize, Option<&str>)> = Vec::new();
    let mut stack: Vec<Option<&str>> = Vec::new();
    let mut at = 0usize;
    for t in tokens {
        let (len, nested) = match t {
            Token::SpanOpen { style, .. } => {
                stack.push(style.as_deref());
                continue;
            }
            Token::SpanClose { .. } => {
                stack.pop();
                continue;
            }
            Token::Text { chars, .. } | Token::Field { chars, .. } => (chars.len(), stack.len() > 1),
            Token::Element(space) => (space.len(), stack.len() > 1),
            Token::Mark { .. } => continue,
        };
        let style = if nested { None } else { stack.last().copied().flatten() };
        segments.push((at, at.saturating_add(len), style));
        at = at.saturating_add(len);
    }
    if at != total {
        return Ok(Vec::new());
    }
    // The runs and the segments both lie in order, so the first segment a run overlaps never comes before the one the run before it began at: one walk of each, and
    // a segment is looked at once for each run that overlaps it (a pass over every segment for each run was a quadratic cost in a paragraph of many runs).
    let (mut first, mut before) = (0usize, 0usize);
    for run in runs {
        if run.start < before {
            first = 0;
        }
        before = run.start;
        while segments.get(first).is_some_and(|(_, to, _)| *to <= run.start) {
            first += 1;
            *steps = steps.saturating_add(1);
        }
        // The style of the first segment the run overlaps, and whether every other it overlaps has it too.
        let (mut only, mut same, mut at): (Option<Option<&str>>, bool, usize) = (None, true, first);
        while let Some((_, to, style)) = segments.get(at).filter(|(from, _, _)| *from < run.end) {
            *steps = steps.saturating_add(1);
            if *to > run.start {
                match only {
                    None => only = Some(*style),
                    Some(held) if held != *style => same = false,
                    Some(_) => {}
                }
            }
            at += 1;
        }
        let named = match only {
            Some(Some(held)) if same => Some(held.to_owned()),
            _ => None,
        };
        out.push(named);
    }
    Ok(out)
}

/// The styles the spans of a paragraph name for the rows of its runs, by the cells of the row (see [`run_style_names`]): the first style a row has, for each row
/// that writes any attribute.
fn collect_reuse(tokens: &[Token], runs: &[runs::Run], reuse: &mut HashMap<String, String>, steps: &mut u64) -> Result<(), WriteError> {
    reuse_from(runs, &run_style_names(tokens, runs, steps)?, reuse);
    Ok(())
}

/// [`collect_reuse`] from the styles of the runs' spans already found.
fn reuse_from(runs: &[runs::Run], names: &[Option<String>], reuse: &mut HashMap<String, String>) {
    for (run, named) in runs.iter().zip(names) {
        let (Some(props), Some(style)) = (run.props.as_deref().filter(|p| p.writes()), named) else { continue };
        reuse.entry(props.key.clone()).or_insert_with(|| style.clone());
    }
}

/// What a paragraph written whole was, for the style of a run whose row changed to be made from the style the file's run had: the old paragraph (its runs and their
/// rows), the style the span of each of its runs names, and the characters the old and the new text share at the head and at the tail.
struct Base<'a> {
    old: &'a View,
    styles: &'a [Option<String>],
    head: usize,
    tail: usize,
    old_len: usize,
    new_len: usize,
    /// The styles of the file the paragraph may edit where they stand ([`Editor::in_place_styles`]).
    in_place: std::collections::HashSet<String>,
}

impl<'a> Base<'a> {
    fn new(old: &'a View, new: &View, styles: &'a [Option<String>]) -> Base<'a> {
        let (old_len, new_len) = (old.chars.chars().count(), new.chars.chars().count());
        let head = old.chars.chars().zip(new.chars.chars()).take_while(|(a, b)| a == b).count();
        let tail =
            old.chars.chars().rev().zip(new.chars.chars().rev()).take(old_len.min(new_len).saturating_sub(head)).take_while(|(a, b)| a == b).count();
        Base { old, styles, head, tail, old_len, new_len, in_place: std::collections::HashSet::new() }
    }

    /// The character of the old text a character of the new text stands in the place of: the same one in the head the two share, the one the same distance from the
    /// end in the tail, and the first that was changed in between.
    fn old_place(&self, new_at: usize) -> Option<usize> {
        let last = self.old_len.checked_sub(1)?;
        let place = if new_at < self.head {
            new_at
        } else if new_at.saturating_add(self.tail) >= self.new_len {
            new_at.saturating_add(self.old_len).saturating_sub(self.new_len)
        } else {
            self.head
        };
        Some(place.min(last))
    }

    /// The style and the row of the run of the old paragraph that the run of the new one starting at the character `start` stands in the place of, when the run
    /// has a row that writes attributes and its whole is in one span that names a style.
    fn run_at(&self, start: usize) -> Option<(&'a str, &'a Props)> {
        let place = self.old_place(start)?;
        let at = self.old.runs.partition_point(|r| r.end <= place);
        let run = self.old.runs.get(at).filter(|r| r.start <= place)?;
        let props = run.props.as_deref().filter(|p| p.writes())?;
        Some((self.styles.get(at)?.as_deref()?, props))
    }
}

/// Whether a start or empty tag in `content` is of the text namespace (by the prefix `text_prefix`) and of a local name `wanted` says yes to: a walk of the
/// tags that maps nothing (the bytes of a paragraph the writer does not map).
///
/// A tag that is not in the bytes or is not text is a fault of the part read ([`WriteError::Mismatch`]), and never a tag that holds nothing (the
/// The paragraph would be taken for one with no element in it, and its elements lost).
fn holds_tag(content: &[u8], text_prefix: &str, wanted: impl Fn(&str) -> bool) -> Result<bool, WriteError> {
    let mut at = 0usize;
    while let Some(lt) = content.get(at..).and_then(|c| c.iter().position(|b| *b == b'<')) {
        let start = at + lt;
        let Some(close) = tag_end(content, start) else { return Ok(false) };
        let tag = content.get(start + 1..close - 1).and_then(|t| std::str::from_utf8(t).ok()).ok_or(WriteError::Mismatch(WORDS_NOT_TEXT))?;
        // `split` gives at least one piece, so the name is the tag's own first word.
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or_default();
        if let Some((prefix, local)) = name.split_once(':')
            && prefix == text_prefix
            && !tag.starts_with('/')
            && wanted(local)
        {
            return Ok(true);
        }
        at = close;
    }
    Ok(false)
}

fn same_key(a: &Option<Arc<Props>>, b: &Option<Arc<Props>>) -> bool {
    a.as_deref().map_or("", |p| p.key.as_str()) == b.as_deref().map_or("", |p| p.key.as_str())
}

/// What an element of the text namespace is called here, by its local name: the ones the writer maps, and `""` for any other.
fn local_name(local: &str) -> &'static str {
    match local {
        "span" => "span",
        "s" => "s",
        "tab" => "tab",
        "line-break" => "line-break",
        other => zero::mark_name(other).or_else(|| zero::field_name(other)).unwrap_or(""),
    }
}

/// Reads back the paragraphs the save rewrote: each is read again from the part made by the same tokens and must be the text the model has.
/// `edited` are the shapes' places in the part made (the page's place in the part and the element's ordinal) with the paragraphs written there.
pub(crate) fn check_paragraphs(
    part: &[u8],
    kind: PartKind,
    (edited, inserted, zeros): (&[super::PagedParas], &[super::PagedStyles], &[super::PagedZero]),
    limits: &Limits,
    work: &mut WriteWork,
) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let ask = Ask { styles: false, inside: edited.iter().map(|(page, ord, _)| (*page, *ord)).collect(), ..Ask::default() };
    let scan = scan_part_asking(part, kind, &ask, limits, work).map_err(|e| WriteError::SelfCheck(format!("the part made does not scan: {e}")))?;
    // The paragraph style each paragraph put in must name, by the shape (indexed once: a look through the list for each shape is quadratic).
    let mut named: HashMap<(usize, u32), &super::StyleNames> = HashMap::new();
    named
        .try_reserve(inserted.len())
        .map_err(|_| WriteError::Limit { what: "memory for the paragraphs a save checks", max: inserted.len() as u64 })?;
    for (page, ord, styles) in inserted {
        named.insert((*page, *ord), styles);
    }
    work.order_steps = work.order_steps.saturating_add(inserted.len() as u64);
    // What each shape expects of the zero-width elements put in its paragraphs, indexed once.
    let mut expected_zero: HashMap<(usize, u32), &Vec<zero::Expect>> = HashMap::new();
    expected_zero
        .try_reserve(zeros.len())
        .map_err(|_| WriteError::Limit { what: "memory for the paragraphs a save checks", max: zeros.len() as u64 })?;
    for (page, ord, list) in zeros {
        expected_zero.insert((*page, *ord), list);
    }
    for (page, ord, paras) in edited {
        let shape = scan.pages.get(*page).and_then(|p| p.shape(*ord)).ok_or_else(|| failed("a shape whose text was written is not in the part"))?;
        let kids: Vec<&Kid> = scan
            .kids
            .get(shape.kids.0 as usize..shape.kids.1 as usize)
            .unwrap_or_default()
            .iter()
            .filter(|k| matches!(k, Kid::Para { .. }))
            .collect();
        for (index, expected) in paras {
            let Some(Kid::Para { open, empty, close_start, item, list, .. }) = kids.get(*index).copied() else {
                return Err(failed("a paragraph written is not in the part"));
            };
            // A paragraph in a list that is not the child of one of its items is one the reader skips (the review).
            if list.is_some() && item.is_none() {
                return Err(failed("a paragraph written is a child of a list and not of one of its items, and the reader does not read it"));
            }
            let content = if *empty { (open.1, open.1) } else { (open.1, *close_start) };
            // Any prefix the part binds to the text namespace: the tokens are told by the local name after the colon.
            let any = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
            let Some((tokens, _)) = tokenize(part, content.0, content.1, &any) else {
                return Err(failed("a paragraph written does not read as text"));
            };
            if read_chars(&tokens, expected.chars().count()).as_deref() != Some(expected.as_str()) {
                return Err(failed("a paragraph written does not read as the text the model has"));
            }
            work.checked_paragraphs = work.checked_paragraphs.saturating_add(1);
        }
        // A paragraph put in has the paragraph style the writer named for it, in the place the model's order gives it: two paragraphs put in the wrong order
        // read alike when their text is the same, and are told apart by their styles.
        for (index, expected) in named.get(&(*page, *ord)).map_or(&[][..], |v| v.as_slice()) {
            let Some(Kid::Para { open, .. }) = kids.get(*index).copied() else {
                return Err(failed("a paragraph put in is not in the part"));
            };
            work.charge_attribute_bytes(open.1.saturating_sub(open.0))?;
            work.order_steps = work.order_steps.saturating_add(1);
            let mut spans: Vec<AttrSpan> = Vec::new();
            let tag = tag_attributes(part, open.0, open.1, &mut spans).ok_or_else(|| failed("a paragraph put in has a tag that does not read"))?;
            let name = part.get(open.0 + 1..tag.name_end).and_then(|n| std::str::from_utf8(n).ok()).unwrap_or_default();
            let wanted = format!("{}:style-name", name.split_once(':').map_or("", |(prefix, _)| prefix));
            let found = spans
                .iter()
                .find(|a| part.get(a.name.0..a.name.1) == Some(wanted.as_bytes()))
                .and_then(|a| part.get(a.value.0..a.value.1))
                .map(crate::package::unescape);
            if found.as_deref() != expected.as_deref() {
                return Err(failed(
                    "a paragraph put in has another paragraph style than the writer named for it, so it is not in the place the model has it (or is not as the model has it)",
                ));
            }
            work.checked_inserted = work.checked_inserted.saturating_add(1);
        }
        for expect in expected_zero.get(&(*page, *ord)).map_or(&[][..], |v| v.as_slice()) {
            check_zero(part, &kids, expect, work)?;
        }
    }
    Ok(())
}

/// Reads back the zero-width elements and fields of one paragraph: the paragraph the expectation names is read again from the part made, and holds the
/// elements the save put in it, each at its offset with its bytes, in order, and no other.
fn check_zero(part: &[u8], kids: &[&Kid], expect: &zero::Expect, work: &mut WriteWork) -> Result<(), WriteError> {
    let failed = |words: &str| WriteError::SelfCheck(words.to_owned());
    let index = match expect {
        zero::Expect::Exact { index, .. } | zero::Expect::Edges { index, .. } => *index,
    };
    let Some(Kid::Para { open, empty, close_start, .. }) = kids.get(index).copied() else {
        return Err(failed("a paragraph that took a zero-width element is not in the part"));
    };
    let content = if *empty { (open.1, open.1) } else { (open.1, *close_start) };
    let any = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
    let Some((tokens, _)) = tokenize(part, content.0, content.1, &any) else {
        return Err(failed("a paragraph that took a zero-width element does not read as text"));
    };
    let Some(read) = reading(&tokens, stands_for(&tokens), false) else {
        return Err(failed("a paragraph that took a zero-width element does not read as text"));
    };
    work.zero_steps = work.zero_steps.saturating_add(read.placed.len() as u64);
    work.checked_zero = work.checked_zero.saturating_add(1);
    // Each element the paragraph holds: where it stands and its bytes.
    let mut found: Vec<(usize, &str)> = Vec::new();
    found.try_reserve_exact(read.placed.len()).map_err(|_| WriteError::Limit { what: "memory for the paragraphs a save checks", max: 0 })?;
    for &(t, at, _) in &read.placed {
        let bytes = match tokens.get(t) {
            Some(Token::Mark { bytes } | Token::Field { bytes, .. }) => *bytes,
            _ => return Err(failed("a zero-width element the reading placed is not among the tokens")),
        };
        let text = part.get(bytes.0..bytes.1).and_then(|b| std::str::from_utf8(b).ok()).ok_or_else(|| failed("a zero-width element is not text"))?;
        found.push((at, text));
    }
    match expect {
        zero::Expect::Exact { items, .. } => {
            let same = found.len() == items.len() && found.iter().zip(items).all(|((at, text), (want_at, want))| at == want_at && text == want);
            if !same {
                return Err(failed("a paragraph written does not hold the zero-width elements the model's text puts in it, each where it stands"));
            }
        }
        zero::Expect::Edges { start, end, .. } => {
            let (head, tail) = (found.get(..start.len()), found.len().checked_sub(end.len()).and_then(|k| found.get(k..)));
            let head_ok = head.is_some_and(|h| h.iter().zip(start).all(|((at, text), want)| *at == 0 && text == want));
            let tail_ok = tail.is_some_and(|t| t.iter().zip(end).all(|((at, text), want)| *at == read.total && text == want));
            if !(head_ok && tail_ok) {
                return Err(failed("a paragraph that kept its bytes does not hold the zero-width elements put in at its start and its end"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diacraft_doc::{Text, TextPart};

    fn text(parts: Vec<TextPart>) -> Arc<Text> {
        Arc::new(Text { parts, ..Default::default() })
    }

    /// A field written again is its start tag and end tag as they are around the new characters, escaped; the start tag ends at the first `>` outside a
    /// quoted value, and an element with no content to replace gives none.
    #[test]
    fn a_refreshed_field_is_its_own_tags_around_the_new_characters() {
        assert_eq!(refreshed_field("<text:page-number>3</text:page-number>", "12").as_deref(), Some("<text:page-number>12</text:page-number>"));
        assert_eq!(
            refreshed_field(r#"<t:file-name t:display="x>y" a='>'>old<t:s/></t:file-name>"#, "a<b&c").as_deref(),
            Some(r#"<t:file-name t:display="x>y" a='>'>a&lt;b&amp;c</t:file-name>"#)
        );
        assert_eq!(refreshed_field("<text:page-number/>", "1"), None);
        assert_eq!(refreshed_field("<text:page-number>3", "1"), None);
        assert_eq!(refreshed_field("<text:page-number a=\"", "1"), None);
        assert_eq!(refreshed_field("", "1"), None);
    }

    fn chars(s: &str) -> TextPart {
        TextPart::Chars(Arc::from(s))
    }

    /// The one walk of the index of styles counts every key it asks about, whatever it keeps, and the counts add up.
    #[test]
    fn a_walk_of_the_index_of_styles_counts_each_key_it_asks_about() {
        let mut keys = StyleKeys::new((0..5).map(|k| (format!("key{k}"), k)).collect());
        assert_eq!((keys.len(), keys.walked), (5, 0), "it reads as the map it holds, and no walk has been made");
        keys.retain(|_, place| *place != 2);
        assert_eq!((keys.len(), keys.walked, keys.get("key2")), (4, 5, None), "a key dropped, five asked about");
        keys.retain(|_, _| true);
        assert_eq!((keys.len(), keys.walked), (4, 9), "four more asked about, none dropped");
        keys.retain(|_, _| false);
        assert_eq!((keys.len(), keys.walked), (0, 13));
        assert_eq!(StyleKeys::default().walked, 0);
    }

    /// The walks of every family's index are what a save is charged at the end of its part: the walks of the families add, and a family never walked adds none.
    #[test]
    fn the_walks_of_every_family_s_index_add_up_for_the_save() {
        let mut texts = TextStyles::default();
        assert_eq!(texts.index_walks(), 0, "no index, no walk");
        for (family, keys) in [(Fam::Paragraph, 3usize), (Fam::Text, 7), (Fam::Graphic, 11)] {
            let mut index = StyleKeys::new((0..keys).map(|k| (format!("key{k}"), k)).collect());
            if family != Fam::Graphic {
                index.retain(|_, place| *place != 0);
                index.retain(|_, _| true);
            }
            texts.index.insert(family, index);
        }
        // Paragraph: 3 + 2; text: 7 + 6; graphic: never walked.
        assert_eq!(texts.index_walks(), 3 + 2 + 7 + 6);
    }

    #[test]
    fn the_paragraphs_written_are_read_back_and_must_read_as_the_text_the_model_has() {
        let part = concat!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
            r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0">"#,
            r#"<office:body><office:drawing><draw:page draw:name="p"><draw:custom-shape svg:width="1cm" svg:height="1cm">"#,
            r#"<text:p>hello<text:tab/>you</text:p><text:p/></draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>"#
        );
        let check = |shape: u32, paras: Vec<(usize, String)>| {
            check_paragraphs(part.as_bytes(), PartKind::Content, (&[(0, shape, paras)], &[], &[]), &Limits::default(), &mut WriteWork::default())
        };
        assert!(check(1, vec![(0, "hello\tyou".into()), (1, String::new())]).is_ok());
        let why = |r: Result<(), WriteError>| match r {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("expected a refusal: {other:?}"),
        };
        assert!(why(check(1, vec![(0, "hullo\tyou".into())])).contains("does not read as the text"));
        assert!(why(check(1, vec![(2, "x".into())])).contains("not in the part"));
        assert!(why(check(9, vec![(0, "x".into())])).contains("not in the part"));
    }

    /// The check of the paragraphs written, each by itself (parked 2): a `text:p` that is the child of a list and not of a list
    /// item is refused (the reader skips it), and a paragraph put in must name the paragraph style the writer named for it.
    #[test]
    fn a_paragraph_in_a_list_that_is_no_items_child_and_a_paragraph_with_another_style_are_refused() {
        let head = concat!(
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" "#,
            r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0">"#,
            r#"<office:body><office:drawing><draw:page draw:name="p"><draw:custom-shape svg:width="1cm" svg:height="1cm">"#
        );
        let tail = "</draw:custom-shape></draw:page></office:drawing></office:body></office:document-content>";
        let why = |r: Result<(), WriteError>| match r {
            Err(WriteError::SelfCheck(words)) => words,
            other => panic!("expected a refusal: {other:?}"),
        };
        let check = |body: &str, paras: Vec<(usize, String)>, styles: Vec<(usize, Option<String>)>| {
            let part = format!("{head}{body}{tail}");
            check_paragraphs(
                part.as_bytes(),
                PartKind::Content,
                (&[(0, 1, paras)], &[(0, 1, styles)], &[]),
                &Limits::default(),
                &mut WriteWork::default(),
            )
        };
        let one = || vec![(0, "x".to_owned())];
        // A paragraph that is the child of a list item is read; the child of the list itself is not.
        assert!(check("<text:list><text:list-item><text:p>x</text:p></text:list-item></text:list>", one(), Vec::new()).is_ok());
        let words = why(check("<text:list><text:p>x</text:p></text:list>", one(), Vec::new()));
        assert!(words.contains("child of a list") && words.contains("reader does not read"), "{words}");
        // The style a paragraph put in names: the same, another, none where one was written, one where none was.
        let styled = "<text:p text:style-name=\"P2\">x</text:p>";
        assert!(check(styled, one(), vec![(0, Some("P2".to_owned()))]).is_ok());
        assert!(check("<text:p>x</text:p>", one(), vec![(0, None)]).is_ok());
        for (body, expected) in [(styled, Some("P3")), (styled, None), ("<text:p>x</text:p>", Some("P2"))] {
            let words = why(check(body, one(), vec![(0, expected.map(str::to_owned))]));
            assert!(words.contains("another paragraph style"), "{body} {expected:?}: {words}");
        }
        // A paragraph put in that the part does not hold, and a style named for a paragraph of the text that was not written (no check of its style).
        assert!(why(check(styled, one(), vec![(4, None)])).contains("not in the part"));
        assert!(check(styled, one(), Vec::new()).is_ok());
    }

    #[test]
    fn a_row_of_spaces_is_one_space_and_a_count_except_at_the_edges_where_it_is_all_counted() {
        // A first space is itself, the rest are counted; a row at the start or the end of the paragraph is counted whole. Several rows in one
        // paragraph each find their own place from the characters before them.
        let w = |s: &str| encode(s, "t", "");
        assert_eq!(w("a b"), "a b");
        assert_eq!(w("a  b"), "a <t:s/>b");
        assert_eq!(w("a   b"), "a <t:s t:c=\"2\"/>b");
        assert_eq!(w("a  b  "), "a <t:s/>b<t:s t:c=\"2\"/>");
        assert_eq!(w("  a"), "<t:s t:c=\"2\"/>a");
        assert_eq!(w("a   "), "a<t:s t:c=\"3\"/>");
        assert_eq!(w(" "), "<t:s/>");
        assert_eq!(w("a  b  c   d"), "a <t:s/>b <t:s/>c <t:s t:c=\"2\"/>d");
        assert_eq!(w("  a  b  "), "<t:s t:c=\"2\"/>a <t:s/>b<t:s t:c=\"2\"/>");
    }

    /// The tokens of a span say where its element is in the part, the paragraph being no earlier than the part's start: the open tag's start and the end of the
    /// close tag, or of the empty element (the mutation of `tokenize` and `kept_runs`).
    #[test]
    fn the_tokens_of_a_span_name_the_bytes_of_its_element_in_the_part() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let part = "01234ab<t:span t:style-name=\"T1\">cd</t:span><t:span t:style-name=\"T2\"/>ef";
        let (tokens, structure) = tokenize(part.as_bytes(), 5, part.len(), &ns).unwrap();
        assert!(structure);
        let spans: Vec<&Token> = tokens.iter().filter(|t| matches!(t, Token::SpanOpen { .. } | Token::SpanClose { .. })).collect();
        let first = part.find("<t:span t:style-name=\"T1\">").unwrap();
        let first_end = part.find("</t:span>").unwrap() + "</t:span>".len();
        let second = part.find("<t:span t:style-name=\"T2\"/>").unwrap();
        let second_end = second + "<t:span t:style-name=\"T2\"/>".len();
        assert_eq!(
            spans,
            [
                &Token::SpanOpen { style: Some("T1".to_owned()), at: first },
                &Token::SpanClose { end: first_end },
                &Token::SpanOpen { style: Some("T2".to_owned()), at: second },
                &Token::SpanClose { end: second_end },
            ]
        );
    }

    #[test]
    fn a_paragraph_that_stands_for_more_than_one_and_a_half_times_the_budget_and_eight_is_not_built() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        // `abcd`, a counted run of spaces and `e` stand for 4 + the count + 1 characters; with a budget of 4 the paragraph may stand for 4 + 2 + 8 = 14 before it
        // is built.
        let gate = |count: usize| {
            let content = format!("abcd<text:s text:c=\"{count}\"/>e");
            let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
            (stands_for(&tokens), stands_for_too_much(&tokens, 4))
        };
        assert_eq!(gate(9), (14, false), "4 + 9 + 1 stand for 14, which is not over 14");
        assert_eq!(gate(10), (15, true), "15 are");
        // And what is over the model's paragraph is no paragraph the reader made, built or not: refused.
        let content = "abcd<text:s text:c=\"3\"/>e";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        assert_eq!(read_chars(&tokens, 8).as_deref(), Some("abcd   e"));
        assert_eq!(read_chars(&tokens, 7), None, "one character over the model's");
        // A collapsed space at the end goes; a counted one does not (only white space the reader collapses is dropped).
        let content = "abcd<text:s text:c=\"2\"/> ";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        assert_eq!(read_chars(&tokens, 6).as_deref(), Some("abcd  "));
        // Raw white space is no such amplification: indentation of any length, however many characters, stands for what it reads as.
        let indented = format!("\n{}abcd\n{}", " ".repeat(500), " ".repeat(500));
        let (tokens, _) = tokenize(indented.as_bytes(), 0, indented.len(), &ns).unwrap();
        assert_eq!(stands_for(&tokens), 1 + 4 + 1);
        assert_eq!(read_chars(&tokens, 4).as_deref(), Some("abcd"));
        assert_eq!(collapsed(&[' ', '\t', 'a', ' ', '\n', ' ', 'b']), 4, "a run of white space is one character");
    }

    #[test]
    fn a_white_space_element_is_the_empty_element_and_a_pair_of_tags_or_a_start_tag_alone_is_not_mapped() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        for content in ["<text:tab/>", "<text:line-break/>", "<text:s/>", "<text:s text:c=\"3\"/>"] {
            assert!(tokenize(content.as_bytes(), 0, content.len(), &ns).is_some(), "{content}");
        }
        for content in ["<text:tab>", "<text:tab></text:tab>", "<text:line-break>", "<text:s>", "<text:s text:c=\"3\"></text:s>", "</text:tab>"] {
            assert!(tokenize(content.as_bytes(), 0, content.len(), &ns).is_none(), "{content}");
        }
    }

    #[test]
    fn text_is_written_as_writes_it_and_reads_back_as_it_was() {
        let ns = |n: &str| -> Option<&'static str> {
            Some(match n.split_once(':')?.1 {
                "span" => "span",
                "s" => "s",
                "tab" => "tab",
                "line-break" => "line-break",
                _ => "",
            })
        };
        // `a`, a space, `text:s` of 2, `b`, tab, `c`, line break, `d`.
        let model = "a   b\tc\u{2028}d";
        let written = encode(model, "text", "");
        assert_eq!(written, r#"a <text:s text:c="2"/>b<text:tab/>c<text:line-break/>d"#);
        let (tokens, structure) = tokenize(written.as_bytes(), 0, written.len(), &ns).unwrap();
        assert!(!structure);
        assert_eq!(read_chars(&tokens, 1_000_000).unwrap(), model);
        // Leading, trailing and lone spaces; markup characters; a single space between words is itself.
        for (model, shown) in [
            (" a", "<text:s/>a"),
            ("a ", "a<text:s/>"),
            ("a b", "a b"),
            ("a  b", "a <text:s/>b"),
            ("   ", r#"<text:s text:c="3"/>"#),
            ("<a&b>", "&lt;a&amp;b&gt;"),
        ] {
            let written = encode(model, "text", "");
            assert_eq!(written, shown);
            let (tokens, _) = tokenize(written.as_bytes(), 0, written.len(), &ns).unwrap();
            let back = read_chars(&tokens, 1_000_000).unwrap();
            assert_eq!(back, model, "{shown}");
        }
        assert_eq!(encode("x", "t1", " xmlns:t1=\"u\""), "x");
        assert_eq!(encode("\t", "t1", " xmlns:t1=\"u\""), "<t1:tab xmlns:t1=\"u\"/>");
    }

    #[test]
    fn a_count_of_spaces_is_clamped_as_the_reader_clamps_it_and_never_built_past_what_the_model_says() {
        // Security finding (MEDIUM): `text:c` was parsed up to a million and a string of that many characters built for each element.
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let content = r#"a<text:s text:c="999999"/>b<text:s text:c="0"/><text:s text:c="-4"/><text:s/>"#;
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        let counts: Vec<usize> = tokens.iter().filter_map(|t| if let Token::Element(s) = t { Some(s.len()) } else { None }).collect();
        assert_eq!(counts, [65_536, 1, 1, 1], "clamped to 1 to the reader's bound, whatever the file says");
        assert_eq!(stands_for(&tokens), 1 + 65_536 + 1 + 3);
        // Built only up to the length the model says: one character short of what it stands for is refused, with nothing made.
        let full = 1 + 65_536 + 1 + 3;
        assert_eq!(read_chars(&tokens, full).map(|s| s.chars().count()), Some(full));
        assert!(read_chars(&tokens, 10).is_none(), "a paragraph that stands for far more than the model says is not built");
        // A thousand such elements stand for 65 MB and are counted at once, and the model of a paragraph that small is what is built.
        let many = r#"<text:s text:c="999999"/>"#.repeat(1_000);
        let (tokens, _) = tokenize(many.as_bytes(), 0, many.len(), &ns).unwrap();
        assert_eq!(stands_for(&tokens), 65_536_000);
        assert!(read_chars(&tokens, 1_000).is_none(), "refused by the count, before a string is made of it");
        // What is written is written in elements no longer than the bound, and reads back.
        let spaces = format!("x{}y", " ".repeat(65_536 * 2 + 5));
        let written = encode(&spaces, "text", "");
        assert_eq!(written.matches("<text:s").count(), 3, "{}", written.len());
        assert!(written.contains(r#"text:c="65536""#) && !written.contains("text:c=\"65537\""));
        let (tokens, _) = tokenize(written.as_bytes(), 0, written.len(), &ns).unwrap();
        assert_eq!(read_chars(&tokens, spaces.chars().count()).as_deref(), Some(spaces.as_str()));
    }

    /// The tokens of the elements a paragraph may hold: where each element is in the part, what a field shows, and what the reader makes of them: the offset of
    /// each element in the paragraph's characters (a mark after a collapsed space is after it; one after the white space the paragraph's end takes off is at the end).
    #[test]
    fn the_tokens_of_an_element_name_its_bytes_and_the_reading_gives_the_offset_it_stands_at() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let content =
            "a <t:bookmark t:name=\"B\"/> b<t:title>T&amp;U</t:title><t:reference-mark t:name=\"r\"></t:reference-mark><t:soft-page-break/>";
        let (tokens, structure) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        assert!(structure);
        let at = |needle: &str| content.find(needle).unwrap();
        let marks: Vec<(usize, usize)> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Mark { bytes } => Some(*bytes),
                _ => None,
            })
            .collect();
        let long_form = "<t:reference-mark t:name=\"r\"></t:reference-mark>";
        assert_eq!(
            marks,
            [
                (at("<t:bookmark"), at("<t:bookmark") + "<t:bookmark t:name=\"B\"/>".len()),
                (at("<t:reference-mark"), at("<t:reference-mark") + long_form.len()),
                (at("<t:soft-page-break/>"), content.len()),
            ]
        );
        let field = tokens.iter().find_map(|t| match t {
            Token::Field { bytes, chars, kind } => Some((*bytes, chars.iter().collect::<String>(), *kind)),
            _ => None,
        });
        assert_eq!(
            field,
            Some(((at("<t:title>"), at("<t:reference-mark")), "T&U".to_owned(), "title")),
            "the characters are what the references stand for"
        );
        // The reading: `a b` and `T&U` after it; the mark after the space is at 2, the field at 3 for 3 characters, the marks after it at 6.
        let read = reading(&tokens, 100, true).unwrap();
        assert_eq!(read.chars, "a bT&U");
        let places: Vec<(usize, usize)> = read.placed.iter().map(|(_, at, len)| (*at, *len)).collect();
        assert_eq!(places, [(2, 0), (3, 3), (6, 0), (6, 0)]);
        assert_eq!(read.total, 6);
        // A mark after the white space that ends the paragraph is at the end, as the characters are one fewer than the count at the mark.
        let content = "a <t:bookmark t:name=\"B\"/>";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        let read = reading(&tokens, 100, false).unwrap();
        assert_eq!((read.total, read.placed.iter().map(|p| p.1).collect::<Vec<_>>()), (1, vec![1]));
        // A mark at the start, before white space that goes: at 0.
        let content = " <t:bookmark t:name=\"B\"/> a";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        let read = reading(&tokens, 100, true).unwrap();
        assert_eq!((read.chars.as_str(), read.placed.iter().map(|p| p.1).collect::<Vec<_>>()), ("a", vec![0]));
        // A quote in an attribute holds a `>`; an empty field shows nothing; an element that holds another is none the writer maps.
        let mark = "<t:bookmark t:name=\"a>b\" t:x='c>d'/>";
        let content = format!("{mark}<t:title/>");
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        let [Token::Mark { bytes }, Token::Field { bytes: field, chars, .. }] = tokens.as_slice() else { panic!("{tokens:?}") };
        assert_eq!((*bytes, *field, chars.len()), ((0, mark.len()), (mark.len(), content.len()), 0));
        for content in [
            "<t:title><t:span>x</t:span></t:title>",
            "<t:bookmark>x</t:bookmark>",
            "</t:bookmark>",
            "</t:title>",
            "<t:bookmark t:name=\"a",
            "<t:title>x",
        ] {
            assert!(tokenize(content.as_bytes(), 0, content.len(), &ns).is_none(), "{content}");
        }
    }

    /// An edit spliced into a run of raw text is one only where the paragraph still reads as the model has it (the reader's white space state, which an element does
    /// not move): an edit that takes a text out from before a plain space, or leaves a text ending in a space at the paragraph's end, is written whole.
    #[test]
    fn an_edit_in_place_leaves_the_white_space_after_it_read_as_it_was() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let pfx = Pfx { draw: "d".into(), svg: "s".into(), fo: "f".into(), style: "st".into(), text: "t".into(), declare: String::new() };
        let try_edit = |content: &str, old: &str, new: &str| -> Option<(usize, usize, String)> {
            let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
            let view = |s: &str| View { chars: s.to_owned(), ..View::default() };
            Editor::in_place(content.as_bytes(), &tokens, &view(old), &view(new), &pfx)
        };
        // A word typed in the middle of a text node: in place.
        assert!(try_edit("ab<t:bookmark t:name=\"x\"/>cd", "abcd", "aXbcd").is_some());
        // The word before a plain space taken out: the space would be the paragraph's first, and goes: whole.
        assert!(try_edit("a<t:bookmark t:name=\"x\"/> b", "a b", " b").is_none());
        // The text before the end taken out where a space is left at the end: whole.
        assert!(try_edit("a b<t:bookmark t:name=\"x\"/>", "a b", "a ").is_none());
        // A space typed after a word, beside a bookmark and another space.
        assert!(try_edit("a<t:bookmark t:name=\"x\"/> b", "a b", "aX b").is_some());
        // Typed at the start of a text that is not the paragraph's first thing, after an element that is no text: the place is a boundary, and the paragraph is written
        // whole; at the start of the paragraph's first text, with other things after it, it is in place.
        assert!(try_edit("<t:tab/>cd", "\tcd", "\tXcd").is_none());
        assert!(try_edit("ab<t:bookmark t:name=\"x\"/>cd", "abcd", "Xabcd").is_some());
        assert!(try_edit("<t:bookmark t:name=\"x\"/>ab", "ab", "Xab").is_none(), "after an element that stands first: the place is a boundary");
        assert!(try_edit("ab", "ab", "Xab").is_some(), "a paragraph of one text");
        // A word replaced from the start of a text: in place.
        assert!(try_edit("ab<t:bookmark t:name=\"x\"/>cd", "abcd", "abXd").is_some());
        // A text that starts with a space after a character, or ends with one before the end: the walk and the state decide, as for any other.
        assert!(try_edit("a<t:bookmark t:name=\"x\"/> b c", "a b c", "a b X").is_some());
        assert!(
            try_edit("<t:bookmark t:name=\"x\"/> ab", "ab", "aXb").is_none(),
            "a space the paragraph's start drops: the characters are not one to one"
        );
        assert!(try_edit("ab <t:bookmark t:name=\"x\"/>", "ab", "abX").is_some(), "the space at the end goes, and stays gone");
        // A field before the text edited does not stop the edit, whatever it shows.
        assert!(try_edit("by <t:title>Ann</t:title> today", "by Ann today", "by Ann todayX").is_some());
        assert!(
            try_edit("by <t:title>A  n</t:title> today", "by A n today", "by A n todayX").is_none(),
            "a field whose white space collapses is not one to one"
        );
    }

    /// The milestone-1 rerun's item 3: an edit in place that reaches the paragraph's last character takes the collapsed white space the reader dropped at the paragraph's
    /// end with it (a span's trailing space stayed after the new text); an edit that stops before it, and one with anything after the node but the ends of spans, leave it.
    #[test]
    fn an_edit_in_place_that_reaches_the_paragraphs_last_character_takes_the_white_space_that_ends_it() {
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let pfx = Pfx { draw: "d".into(), svg: "s".into(), fo: "f".into(), style: "st".into(), text: "t".into(), declare: String::new() };
        let try_edit = |content: &str, old: &str, new: &str| -> Option<(usize, usize, String)> {
            let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
            let view = |s: &str| View { chars: s.to_owned(), ..View::default() };
            Editor::in_place(content.as_bytes(), &tokens, &view(old), &view(new), &pfx)
        };
        // `<t:span>` is 8 bytes: the node `Hello ` is bytes 8 to 14, and the model's text is `Hello`.
        let span = "<t:span>Hello </t:span>";
        assert_eq!(try_edit(span, "Hello", "World"), Some((8, 14, "World".to_owned())), "the whole text: the space goes with it");
        assert_eq!(try_edit(span, "Hello", "Help"), Some((11, 14, "p".to_owned())), "the last characters: the space goes with them");
        assert_eq!(try_edit(span, "Hello", "Hello!"), Some((13, 14, "!".to_owned())), "text typed at the end: the space is where it goes");
        assert_eq!(try_edit(span, "Hello", "Hell"), Some((12, 14, String::new())), "the last character taken out");
        assert_eq!(try_edit(span, "Hello", "Jello"), Some((8, 9, "J".to_owned())), "an edit that stops before the last character leaves the space");
        // A run of two spans, the last one's space: the same, in the last.
        let two = "<t:span>Hello</t:span><t:span> there </t:span>";
        assert_eq!(try_edit(two, "Hello there", "Hello you"), Some((31, 37, "you".to_owned())), "the last span's node to its end");
        assert_eq!(try_edit(two, "Hello there", "Jello there"), Some((8, 9, "J".to_owned())), "the first span, which does not reach the end");
        // Anything after the node but the end of a span stops it (a text after the node is no case: the edit would not reach the paragraph's last character).
        let element = "<t:span>Hello </t:span><t:bookmark t:name=\"x\"/>";
        assert_eq!(try_edit(element, "Hello", "World"), Some((8, 13, "World".to_owned())), "an element after: the space stays");
        // A paragraph whose last node holds no white space at its end is unchanged by it.
        assert_eq!(try_edit("<t:span>Hello</t:span>", "Hello", "World"), Some((8, 13, "World".to_owned())));
    }

    #[test]
    fn the_reader_s_white_space_state_counts_what_it_reads_and_moves_on() {
        let word = |s: &str| s.chars().collect::<Vec<char>>();
        // Each character of a word counts, and the state after it is a character; a space counts once after a character and not after a space or at the start.
        let mut state = START;
        assert_eq!(walk(&word("a b"), &mut state), 3);
        assert_eq!(state, (false, false));
        let mut state = START;
        assert_eq!(walk(&word(" a  b "), &mut state), 4, "the leading space and the second of two go; the last space counts");
        assert_eq!(state, (true, true), "a collapsed space is the last character");
        let mut state = START;
        assert_eq!(walk(&word("  \t\n"), &mut state), 0);
        assert_eq!(state, START);
        let mut state = (false, false);
        assert_eq!(walk(&word(" x"), &mut state), 2, "a space after a character counts");
        // A field whose text ends in a space that the paragraph's end takes off is shorter by it: the element at the end shows what is left.
        let ns = |n: &str| -> Option<&'static str> { Some(local_name(n.split_once(':')?.1)) };
        let content = "x<t:title>Ann </t:title>";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        let read = reading(&tokens, 100, true).unwrap();
        assert_eq!((read.chars.as_str(), read.total), ("xAnn", 4));
        assert_eq!(read.placed.iter().map(|p| (p.1, p.2)).collect::<Vec<_>>(), [(1, 3)], "the field shows `Ann` and not `Ann `");
    }

    #[test]
    fn where_a_tag_and_an_end_tag_end_is_told_by_quotes_and_by_the_name() {
        // A `>` in a quoted value (either quote) does not end the tag; the end is after the first `>` outside the quotes; a tag not closed has none.
        assert_eq!(tag_end(b"<a x=\"1>2\" y='3>4'>rest", 0), Some(19));
        assert_eq!(tag_end(b"..<a/>", 2), Some(6));
        assert_eq!(tag_end(b"<a x=\"1>", 0), None);
        assert_eq!(tag_end(b"<a x=1", 0), None);
        assert_eq!(tag_end(b"<a>", 9), None, "a start past the end");
        // An end tag is `</`, the name and `>` at once.
        assert_eq!(end_tag_after(b"</t:a>", 0, "t:a"), Some(6));
        assert_eq!(end_tag_after(b"xx</t:a>yy", 2, "t:a"), Some(8));
        for (bytes, name) in [
            (&b"</t:a >"[..], "t:a"),
            (b"</t:b>", "t:a"),
            (b"xxt:a>", "t:a"),
            (b"<t:a>", "t:a"),
            (b"</t:a", "t:a"),
            (b"</t:aa>", "t:a"),
            (b"", "t:a"),
        ] {
            assert_eq!(end_tag_after(bytes, 0, name), None, "{}", String::from_utf8_lossy(bytes));
        }
        assert_eq!(end_tag_after(b"</t:a>", 9, "t:a"), None, "a start past the end");
    }

    #[test]
    fn content_the_writer_does_not_map_is_none() {
        let ns = |n: &str| -> Option<&'static str> { Some(if n == "text:span" { "span" } else { "" }) };
        for content in ["a<text:a>b</text:a>", "<!-- c -->"] {
            assert!(tokenize(content.as_bytes(), 0, content.len(), &ns).is_none(), "{content}");
        }
        // A reference is read as the character it stands for, and is no raw text to map byte for byte.
        let content = "a &amp; b";
        let (tokens, _) = tokenize(content.as_bytes(), 0, content.len(), &ns).unwrap();
        assert!(matches!(&tokens[0], Token::Text { reference: true, .. }));
        assert_eq!(read_chars(&tokens, 1_000_000).unwrap(), "a & b");
    }

    use diacraft_doc::{Cell, CellName, Row, Section, Value};

    /// A shape whose text is `parts` and whose `Character` rows (`IX` and cells) and `Paragraph` rows are as given.
    fn shape(parts: Vec<TextPart>, characters: &[(u32, &[(&str, f64)])], paragraphs: &[(u32, &[(&str, f64)])]) -> Shape {
        let mut s = Shape::new(1);
        s.text = (!parts.is_empty()).then(|| text(parts));
        let sheet = Arc::make_mut(&mut s.sheet);
        for (name, rows) in [("Character", characters), ("Paragraph", paragraphs)] {
            if rows.is_empty() {
                continue;
            }
            let mut section = Section::new(name, None);
            for (ix, cells) in rows {
                let mut row = Row { ix: Some(*ix), ..Row::default() };
                for (cell, v) in *cells {
                    row.cells.insert(CellName::new(cell), Cell::constant(Value::from_number(*v).unwrap()));
                }
                section.rows.push(Arc::new(row));
            }
            sheet.sections.push(Arc::new(section));
        }
        s
    }

    const BOLD: &[(&str, f64)] = &[("Style", 1.0), ("Size", 0.25)];
    const ITALIC: &[(&str, f64)] = &[("Style", 2.0), ("Size", 0.25)];

    #[test]
    fn a_text_that_is_the_same_by_the_cells_of_its_rows_is_no_edit_whatever_the_ix_of_the_rows_are() {
        let a = shape(vec![chars("ab"), TextPart::Cp(1), chars("cd")], &[(0, &[]), (1, BOLD)], &[]);
        let same = shape(vec![chars("ab"), TextPart::Cp(7), chars("cd")], &[(0, &[]), (7, BOLD)], &[]);
        assert!(
            text_edit(&a, &same, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().is_none(),
            "the engine numbers the rows of a text anew when it formats part of it"
        );
        // The same characters under rows whose cells differ are an edit; so is a change of a cell of a row the text uses.
        let other = shape(vec![chars("ab"), TextPart::Cp(1), chars("cd")], &[(0, &[]), (1, ITALIC)], &[]);
        let edit = text_edit(&a, &other, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().unwrap();
        assert_eq!(edit.steps, [Step::Edit { old: 0, new: 0 }]);
        // A row nothing in the text uses, changed to cells the old rows have none like, is a format with no character to carry it: refused, and not no edit. The
        // same rows renumbered, or the cells of a row it already had, are no change.
        let a = shape(vec![chars("ab")], &[(0, &[]), (5, BOLD)], &[]);
        let b = shape(vec![chars("ab")], &[(0, &[]), (5, ITALIC)], &[]);
        assert!(text_edit(&a, &b, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap_err().contains("no character"));
        let renumbered = shape(vec![chars("ab")], &[(0, &[]), (9, BOLD)], &[]);
        assert!(text_edit(&a, &renumbered, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().is_none());
        let fewer = shape(vec![chars("ab")], &[(0, &[])], &[]);
        assert!(
            text_edit(&a, &fewer, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().is_none(),
            "a row gone is no format with nothing to carry it"
        );
    }

    #[test]
    fn a_retype_inside_a_run_is_one_edited_paragraph_and_the_others_are_kept() {
        let old = shape(vec![chars("one\ntwo\nthree")], &[], &[]);
        let new = shape(vec![chars("one\ntwx\nthree")], &[], &[]);
        let edit = text_edit(&old, &new, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().unwrap();
        assert_eq!(edit.steps, [Step::Keep { old: 0, new: 0 }, Step::Edit { old: 1, new: 1 }, Step::Keep { old: 2, new: 2 }]);
        assert_eq!(edit.written(), [(1, "twx".to_owned())]);
        assert!(edit.old[0].chars.is_empty() && edit.new[2].chars.is_empty(), "what is kept is let go");
        assert!(edit.holds_text());
        // Nothing is text but an empty paragraph written: no text to hold.
        let blank = shape(vec![chars("one\n")], &[], &[]);
        let edit = text_edit(&old, &blank, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().unwrap();
        assert!(edit.steps.iter().any(|s| matches!(s, Step::Remove { .. })));
    }

    #[test]
    fn a_paragraph_added_or_taken_away_is_an_insert_or_a_remove() {
        let old = shape(vec![chars("a\nc")], &[], &[]);
        let more = shape(vec![chars("a\nb\nc")], &[], &[]);
        let edit = text_edit(&old, &more, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().unwrap();
        assert_eq!(edit.steps, [Step::Keep { old: 0, new: 0 }, Step::Insert { new: 1 }, Step::Keep { old: 1, new: 2 }]);
        assert_eq!(edit.written(), [(1, "b".to_owned())]);
        let edit = text_edit(&more, &old, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap().unwrap();
        assert_eq!(edit.steps, [Step::Keep { old: 0, new: 0 }, Step::Remove { old: 1 }, Step::Keep { old: 2, new: 1 }]);
        assert!(edit.written().is_empty() && !edit.holds_text());
    }

    #[test]
    fn a_row_in_force_holds_across_paragraphs_and_the_last_pp_before_a_paragraph_s_end_sets_it() {
        // `cp` 1 holds from the start to the end of both paragraphs: both are one run of the bold row.
        let s = shape(vec![TextPart::Cp(1), chars("ab\ncd")], &[(0, &[]), (1, BOLD)], &[]);
        let views = {
            let (mut c, mut p) = (Rows::of(&s.sheet, false, &runs::no_palette, &mut 0), Rows::of(&s.sheet, true, &runs::no_palette, &mut 0));
            runs::views(s.text.as_ref(), &mut c, &mut p, &mut 0).unwrap()
        };
        assert_eq!(views.len(), 2);
        for v in &views {
            assert_eq!(v.runs.len(), 1);
            assert!(v.runs[0].props.is_some());
        }
        // `pp` 1 stands at the end of the first paragraph (the last one before a paragraph's end sets the whole of it) and holds for the second.
        let aligned: &[(&str, f64)] = &[("HorzAlign", 1.0)];
        let s = shape(vec![chars("ab"), TextPart::Pp(1), chars("\ncd")], &[], &[(0, &[]), (1, aligned)]);
        let views = {
            let (mut c, mut p) = (Rows::of(&s.sheet, false, &runs::no_palette, &mut 0), Rows::of(&s.sheet, true, &runs::no_palette, &mut 0));
            runs::views(s.text.as_ref(), &mut c, &mut p, &mut 0).unwrap()
        };
        assert!(views.iter().all(|v| v.pp.as_deref().is_some_and(|p| p.key.contains("HorzAlign"))));
        // A text with no marker has row 0, and no style when row 0 has no cell.
        let plain = shape(vec![chars("ab")], &[(0, &[])], &[]);
        let views = {
            let (mut c, mut p) = (Rows::of(&plain.sheet, false, &runs::no_palette, &mut 0), Rows::of(&plain.sheet, true, &runs::no_palette, &mut 0));
            runs::views(plain.text.as_ref(), &mut c, &mut p, &mut 0).unwrap()
        };
        assert!(views[0].runs.is_empty() && views[0].pp.is_none());
    }

    #[test]
    fn what_the_writer_does_not_write_is_refused_in_words() {
        let ok = shape(vec![chars("ab")], &[], &[]);
        let words = |new: Shape| text_edit(&ok, &new, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap_err();
        assert!(words(shape(vec![TextPart::Tp(0), chars("ab")], &[], &[])).contains("tab stop marker"));
        assert!(words(shape(vec![TextPart::Cp(0), chars("x")], &[(0, &[("Locale", 3.0)])], &[])).contains("no attribute of a text style"));
        assert!(words(shape(vec![TextPart::Pp(0), chars("x")], &[], &[(0, &[("Tabs", 3.0)])])).contains("no attribute of a paragraph style"));
        assert!(words(shape(vec![TextPart::Cp(0), chars("x")], &[(0, &[("Style", 300.0)])], &[])).contains("value its attribute cannot hold"));
        // A cell that holds a formula is tier 2.
        let mut formula = shape(vec![TextPart::Cp(0), chars("x")], &[(0, &[("Size", 0.2)])], &[]);
        {
            let sheet = Arc::make_mut(&mut formula.sheet);
            let section = Arc::make_mut(sheet.sections.get_mut(0).unwrap());
            let row = Arc::make_mut(section.rows.get_mut(0).unwrap());
            row.cells.insert(CellName::new("Size"), Cell::read(Some("1/4"), Some("0.25"), None, None, &mut 1_000));
        }
        assert!(words(formula).contains("formula"));
        // A text of more than 16 MiB, and of more paragraphs than a retext holds.
        let big = shape(vec![TextPart::Chars(Arc::from("x".repeat(MOST_EDITED_TEXT + 1)))], &[], &[]);
        assert!(text_edit(&ok, &big, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap_err().contains("16 MiB"));
        let lines = shape(vec![TextPart::Chars(Arc::from("\n".repeat(runs::MOST_PARAGRAPHS)))], &[], &[]);
        assert!(text_edit(&ok, &lines, &runs::no_palette, &mut 0, &mut runs::all_the_room()).unwrap_err().contains("2,000,000 paragraphs"));
        // The cells a row's attributes are written from: bold, italic, underline, strike, size, colour.
        let cells: &[(&str, f64)] = &[("Style", 7.0), ("Size", 14.0 / 72.0), ("Strikethru", 1.0), ("DblUnderline", 0.0), ("Case", 1.0)];
        let rich = shape(vec![TextPart::Cp(0), chars("x")], &[(0, cells)], &[]);
        let (mut c, mut p) = (Rows::of(&rich.sheet, false, &runs::no_palette, &mut 0), Rows::of(&rich.sheet, true, &runs::no_palette, &mut 0));
        let views = runs::views(rich.text.as_ref(), &mut c, &mut p, &mut 0).unwrap();
        let props = views[0].runs[0].props.clone().unwrap();
        let attrs: Vec<(String, String)> = props
            .ops
            .iter()
            .map(|o| {
                (
                    o.local.to_owned(),
                    match &o.val {
                        Val::Text(t) => t.clone(),
                        other => format!("{other:?}"),
                    },
                )
            })
            .collect();
        let get = |n: &str| attrs.iter().find(|(l, _)| l == n).map(|(_, v)| v.as_str());
        assert_eq!(get("font-weight"), Some("bold"));
        assert_eq!(get("font-style"), Some("italic"));
        assert_eq!(get("font-size"), Some("14pt"), "A size of 14 pt is written in points");
        assert_eq!(get("text-underline-style"), Some("solid"));
        assert_eq!(get("text-underline-type"), Some("single"));
        assert_eq!(get("text-line-through-style"), Some("solid"));
        assert_eq!(get("text-transform"), Some("uppercase"));
    }

    /// A text of `n` characters with the room for them asked for as `text_chars` asks it (one for each byte of raw text).
    fn text_token(n: usize) -> Token {
        let mut chars: Vec<char> = Vec::new();
        chars.try_reserve_exact(n).expect("room");
        chars.extend(std::iter::repeat_n('a', n));
        Token::Text { span: (0, n), chars, reference: false }
    }

    /// A token is its own size, and a text or a field also the room its characters were asked for, four bytes each and the allocation's own.
    #[test]
    fn a_paragraph_s_tokens_hold_a_token_each_and_four_bytes_for_each_character_room_of_a_text_or_a_field() {
        let token = size_of::<Token>() as u64;
        assert_eq!(tokens_bytes(&[]), 0);
        for plain in [Token::SpanClose { end: 3 }, Token::Mark { bytes: (0, 4) }, Token::SpanOpen { style: None, at: 0 }] {
            assert_eq!(tokens_bytes(std::slice::from_ref(&plain)), token, "{plain:?} holds no characters");
        }
        let text = text_token(10);
        let Token::Text { chars, .. } = &text else { panic!("a text") };
        let room = chars.capacity() as u64;
        assert!(room >= 10);
        assert_eq!(tokens_bytes(std::slice::from_ref(&text)), token + room * 4 + ALLOCATION_BYTES);
        let field = Token::Field { bytes: (0, 9), chars: vec!['t'; 5], kind: "title" };
        let Token::Field { chars, .. } = &field else { panic!("a field") };
        let room_of_field = chars.capacity() as u64;
        assert_eq!(tokens_bytes(std::slice::from_ref(&field)), token + room_of_field * 4 + ALLOCATION_BYTES);
        // The sum of every token of a paragraph, and the room of a text asked for is what is held, not the characters it came to.
        let all = [text, Token::Mark { bytes: (0, 4) }, field, text_token(0)];
        let one = |t: &Token| tokens_bytes(std::slice::from_ref(t));
        assert_eq!(tokens_bytes(&all), all.iter().map(one).sum::<u64>());
        assert_eq!(one(&text_token(0)), token + ALLOCATION_BYTES, "an empty text still has its allocation counted");
        let wide = text_token(1000);
        assert!(one(&wide) >= token + 4000 + ALLOCATION_BYTES, "four bytes for each character of room");
    }

    /// A range that is not in the text, or a tag that is not text, is a fault and never an empty piece or a tag that holds nothing.
    #[test]
    fn a_range_that_is_not_in_the_text_is_a_fault_and_not_an_empty_piece() {
        // The characters a paragraph's text is read in, one place for each.
        let text = "aéb";
        let mut chars = text.char_indices().peekable();
        assert_eq!(take(text, &mut chars, 1).unwrap(), "a");
        assert_eq!(take(text, &mut chars, 1).unwrap(), "é");
        assert_eq!(take(text, &mut chars, 0).unwrap(), "", "no characters is an empty piece");
        assert_eq!(take(text, &mut chars, 5).unwrap(), "b", "past the end is what is left");
        assert_eq!(take(text, &mut chars, 1).unwrap(), "", "and then nothing");
        // A walk of the places of another text, which put the end inside a character of this one: no piece.
        let mut off = "aéb".char_indices().peekable();
        assert!(matches!(take("éaab", &mut off, 0), Ok("")), "an empty piece at a place that is a boundary");
        let mut off = "aéb".char_indices().peekable();
        off.next();
        assert!(matches!(take("éaab", &mut off, 1), Err(WriteError::Mismatch(_))), "a piece that ends inside a character");
        // A tag that is not text is a fault; one that is, is looked at.
        let wanted = |local: &str| local == "bookmark";
        assert_eq!(holds_tag(b"a<text:bookmark text:name=\"x\"/>b", "text", wanted).ok(), Some(true));
        assert_eq!(holds_tag(b"a<text:span>b</text:span>", "text", wanted).ok(), Some(false));
        assert_eq!(holds_tag(b"</text:bookmark>", "text", wanted).ok(), Some(false), "an end tag is no element");
        assert_eq!(holds_tag(b"<other:bookmark/>", "text", wanted).ok(), Some(false), "another namespace");
        assert_eq!(holds_tag(b"no tag at all", "text", wanted).ok(), Some(false));
        assert_eq!(holds_tag(b"<text:bookmark", "text", wanted).ok(), Some(false), "a tag that is not closed is left to the tokenizer");
        assert!(matches!(holds_tag(b"<text:book\xFFmark/>", "text", wanted), Err(WriteError::Mismatch(_))), "a tag that is not text");
        assert!(matches!(holds_tag(b"<text:bookmark/><text:\xFF/>", "text", wanted), Ok(true)), "found before the tag that is not text");
        assert!(matches!(holds_tag(b"<a/><\xFF>", "text", wanted), Err(WriteError::Mismatch(_))), "a bad tag after a good one");
    }

    /// A paragraph of `chars` with runs `(start, end, key)`; a key that is empty is a run with no row.
    fn paragraph(chars: &str, runs: &[(usize, usize, &str)]) -> View {
        let props = |key: &str| {
            (!key.is_empty()).then(|| {
                Arc::new(Props {
                    key: key.to_owned(),
                    ops: vec![Op { prop: 2, ns: Ns::Fo, local: "font-size", val: Val::Text("1pt".to_owned()) }],
                    faces: Vec::new(),
                    bullet: false,
                    list: None,
                    parts: Vec::new(),
                })
            })
        };
        View {
            chars: chars.to_owned(),
            runs: runs.iter().map(|(start, end, key)| runs::Run { start: *start, end: *end, props: props(key) }).collect(),
            ..View::default()
        }
    }

    #[test]
    fn a_run_of_the_new_text_stands_where_the_head_and_the_tail_the_two_texts_share_put_it() {
        // `ab|cd` against `abX|cYd`: the head is `ab`, the tail `d`; the runs of the old text are `ab` and `cd`.
        let old = paragraph("abcd", &[(0, 2, "r0"), (2, 4, "r1")]);
        let names = vec![Some("T0".to_owned()), Some("T1".to_owned())];
        let new = paragraph("abXcYd", &[(0, 3, "r0"), (3, 6, "r1")]);
        let base = Base::new(&old, &new, &names);
        assert_eq!((base.head, base.tail, base.old_len, base.new_len), (2, 1, 4, 6));
        // In the head: the same place.
        assert_eq!(base.old_place(0), Some(0));
        assert_eq!(base.old_place(1), Some(1));
        // In the middle, which was changed: the first place that was.
        assert_eq!(base.old_place(2), Some(2));
        assert_eq!(base.old_place(3), Some(2));
        assert_eq!(base.old_place(4), Some(2));
        // In the tail: the same distance from the end.
        assert_eq!(base.old_place(5), Some(3));
        // The run of the new text that starts at 3 stands in the place of the old `cd`, whose style and row it takes.
        let (style, props) = base.run_at(3).unwrap();
        assert_eq!((style, props.key.as_str()), ("T1", "r1"));
        let (style, props) = base.run_at(0).unwrap();
        assert_eq!((style, props.key.as_str()), ("T0", "r0"));
        // A text with a tail that reaches the front: `abc` against `xabc`, the head 0 and the tail 3.
        let (old, new) = (paragraph("abc", &[(0, 3, "r0")]), paragraph("xabc", &[(0, 4, "r0")]));
        let base = Base::new(&old, &new, &names);
        assert_eq!((base.head, base.tail), (0, 3));
        assert_eq!((base.old_place(0), base.old_place(1), base.old_place(3)), (Some(0), Some(0), Some(2)));
        // A place past the end of the old text is its last character.
        let (old, new) = (paragraph("ab", &[(0, 2, "r0")]), paragraph("abcdef", &[(0, 6, "r0")]));
        let base = Base::new(&old, &new, &names);
        assert_eq!((base.head, base.tail), (2, 0));
        assert_eq!(base.old_place(5), Some(1));
        assert_eq!(base.old_place(2), Some(1));
        // A character taken out: `abcd` against `abd`, the head 2 and the tail 1. The place the head ends at is the first of the tail (the `d`), not the changed `c`.
        let (old, new) = (paragraph("abcd", &[(0, 2, "r0"), (2, 3, "r1"), (3, 4, "r2")]), paragraph("abd", &[(0, 2, "r0"), (2, 3, "r2")]));
        let names3 = vec![Some("T0".to_owned()), Some("T1".to_owned()), Some("T2".to_owned())];
        let base = Base::new(&old, &new, &names3);
        assert_eq!((base.head, base.tail), (2, 1));
        assert_eq!((base.old_place(1), base.old_place(2)), (Some(1), Some(3)));
        assert_eq!(base.run_at(2).map(|r| r.0), Some("T2"));
        // A text that repeats itself, `ab` against `abab`: the tail the two share is what is left after the head and is not counted over it, so the characters put in
        // stand at the first place of the old text the head did not take, clamped to its last character (the tail counted over the head would give place 0).
        let (old, new) = (paragraph("ab", &[(0, 1, "r0"), (1, 2, "r1")]), paragraph("abab", &[(0, 4, "r0")]));
        let names2 = vec![Some("T0".to_owned()), Some("T1".to_owned())];
        let base = Base::new(&old, &new, &names2);
        assert_eq!((base.head, base.tail), (2, 0));
        assert_eq!((base.old_place(2), base.old_place(3)), (Some(1), Some(1)));
        // An old text of no characters has no run to stand in the place of.
        let (old, new) = (paragraph("", &[]), paragraph("ab", &[(0, 2, "r0")]));
        let base = Base::new(&old, &new, &names);
        assert_eq!((base.old_place(0), base.run_at(0).map(|r| r.0)), (None, None));
    }

    #[test]
    fn a_run_is_found_by_the_place_it_stands_in_and_has_a_style_only_when_the_old_run_has_a_row_that_writes_and_a_span_that_names_a_style() {
        let old = paragraph("abcdef", &[(0, 2, "r0"), (2, 4, ""), (4, 6, "r2")]);
        let new = paragraph("abcdef", &[(0, 6, "x")]);
        // The first run has a span, the second no row, the third a row and no span that names one.
        let names = vec![Some("T0".to_owned()), None, None];
        let base = Base::new(&old, &new, &names);
        assert_eq!(base.run_at(0).map(|r| r.0), Some("T0"));
        assert_eq!(base.run_at(1).map(|r| r.0), Some("T0"), "the last character of the run");
        assert_eq!(base.run_at(2).map(|r| r.0), None, "a run with no row");
        assert_eq!(base.run_at(3).map(|r| r.0), None);
        assert_eq!(base.run_at(4).map(|r| r.0), None, "a row and no style");
        // The names are one for each run: a list that is short names none.
        let short = Vec::new();
        let base = Base::new(&old, &new, &short);
        assert_eq!(base.run_at(0).map(|r| r.0), None);
    }

    fn token(style: Option<&str>) -> Token {
        Token::SpanOpen { style: style.map(str::to_owned), at: 0 }
    }

    fn chars_token(n: usize) -> Token {
        Token::Text { span: (0, 0), chars: vec!['x'; n], reference: false }
    }

    #[test]
    fn the_style_of_each_run_is_the_style_of_the_one_span_it_lies_in_and_nothing_for_any_other_run() {
        let run = |start, end| runs::Run { start, end, props: None };
        let close = Token::SpanClose { end: 0 };
        // `a` outside any span, `bc` in a span of T1, `de` in a span of T2 inside one of T3 (two spans: no style).
        let tokens = vec![
            chars_token(1),
            token(Some("T1")),
            chars_token(2),
            close.clone(),
            token(Some("T3")),
            token(Some("T2")),
            chars_token(2),
            close.clone(),
            close,
        ];
        let runs = [run(0, 1), run(1, 3), run(3, 5)];
        assert_eq!(names_of(&tokens, &runs), [None, Some("T1".to_owned()), None], "outside, in one span, in two spans");
        // Across two spans, across a span and the text before it, across the whole, and two runs in one span.
        assert_eq!(names_of(&tokens, &[run(0, 3), run(3, 5)]), [None, None]);
        assert_eq!(names_of(&tokens, &[run(0, 1), run(1, 5)]), [None, None]);
        assert_eq!(names_of(&tokens, &[run(0, 5)]), [None]);
        let two = [run(0, 1), run(1, 2), run(2, 3), run(3, 5)];
        assert_eq!(names_of(&tokens, &two), [None, Some("T1".to_owned()), Some("T1".to_owned()), None]);
        // Runs that are not in order are found as they would be by looking at every segment.
        let unordered = [run(3, 5), run(0, 1), run(1, 3), run(1, 2), run(3, 5)];
        assert_eq!(names_of(&tokens, &unordered), [None, None, Some("T1".to_owned()), Some("T1".to_owned()), None]);
        // A text whose characters are not the runs' end: none is given a style.
        assert!(names_of(&tokens, &[run(0, 4)]).is_empty());
        assert!(names_of(&[], &[]).is_empty());
        // A span that names no style gives none.
        let tokens = vec![token(None), chars_token(2), Token::SpanClose { end: 0 }];
        assert_eq!(names_of(&tokens, &[run(0, 2)]), [None]);
    }

    /// The style of each run, the steps not asked for.
    fn names_of(tokens: &[Token], runs: &[runs::Run]) -> Vec<Option<String>> {
        run_style_names(tokens, runs, &mut 0).unwrap_or_default()
    }

    /// A paragraph of `n` one-character spans, alternately of `T0` and `T1`, and a run for each: the style of each run is the span's, in work that is a step or two for
    /// each run and each segment, **whatever the number of them** (a pass over every segment for each run was `n` times `n`).
    #[test]
    fn the_style_of_each_run_of_n_and_4n_spans_is_found_in_steps_that_grow_with_n() {
        let steps_of = |n: usize| {
            let mut tokens: Vec<Token> = Vec::new();
            for k in 0..n {
                tokens.push(token(Some(if k % 2 == 0 { "T0" } else { "T1" })));
                tokens.push(chars_token(1));
                tokens.push(Token::SpanClose { end: 0 });
            }
            let runs: Vec<runs::Run> = (0..n).map(|k| runs::Run { start: k, end: k + 1, props: None }).collect();
            let mut steps = 0u64;
            let names = run_style_names(&tokens, &runs, &mut steps).unwrap_or_default();
            assert_eq!(names.len(), n);
            assert!(names.iter().enumerate().all(|(k, name)| name.as_deref() == Some(if k % 2 == 0 { "T0" } else { "T1" })));
            steps
        };
        let (small, large) = (steps_of(500), steps_of(2_000));
        // A step for each run's segment, and one for each segment passed over: two for each run at most.
        assert!((500..=2 * 500 + 2).contains(&small), "{small}");
        assert!((2_000..=2 * 2_000 + 2).contains(&large), "{large}");
        // Two runs that begin at one place are not a step back: the segment the first one stopped at is the second one's, and nothing is walked again (a step for the
        // segment passed over and a step for each looked at: 1 + 1 + 1 + 1).
        let tokens =
            vec![token(Some("T0")), chars_token(1), Token::SpanClose { end: 0 }, token(Some("T1")), chars_token(1), Token::SpanClose { end: 0 }];
        let runs =
            [runs::Run { start: 0, end: 1, props: None }, runs::Run { start: 1, end: 2, props: None }, runs::Run { start: 1, end: 2, props: None }];
        let mut steps = 0u64;
        assert_eq!(run_style_names(&tokens, &runs, &mut steps).ok(), Some(vec![Some("T0".to_owned()), Some("T1".to_owned()), Some("T1".to_owned())]));
        assert_eq!(steps, 4);
        // Runs that are not in order are looked for from the front again, and cost what that costs, the answer the same.
        let tokens = vec![token(Some("T0")), chars_token(2), Token::SpanClose { end: 0 }];
        let runs =
            [runs::Run { start: 1, end: 2, props: None }, runs::Run { start: 0, end: 1, props: None }, runs::Run { start: 1, end: 2, props: None }];
        assert_eq!(names_of(&tokens, &[runs[0].clone(), runs[1].clone(), runs[2].clone()]), vec![Some("T0".to_owned()); 3]);
    }

    /// The reuse of a style by the row it was made for: the first style a row has, and no row that writes nothing.
    #[test]
    fn the_style_a_row_has_is_the_first_one_a_span_names_for_it() {
        let run = |start, end, key: &str| paragraph("", &[(start, end, key)]).runs.remove(0);
        let runs = [run(0, 1, "a"), run(1, 2, "a"), run(2, 3, "b"), run(3, 4, "")];
        let names = [Some("T1".to_owned()), Some("T2".to_owned()), None, Some("T4".to_owned())];
        let mut reuse = HashMap::new();
        reuse_from(&runs, &names, &mut reuse);
        let mut got: Vec<_> = reuse.into_iter().collect();
        got.sort();
        assert_eq!(
            got,
            [("a".to_owned(), "T1".to_owned())],
            "the first of a row's, none for a run with no span's style, none for a row that writes nothing"
        );
    }
}
