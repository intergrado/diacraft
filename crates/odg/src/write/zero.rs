//! Zero-width elements and kept fields in a paragraph written whole.
//!
//! A bookmark, a reference mark (each a point or a range's two ends), a soft page break and a field the model holds as plain text hold no characters or only
//! characters the model has, and the model has no place for the element: it keeps the element's bytes, and when its paragraph is written whole the writer
//! puts the element back, where R10 says. This module is the part of that rule that needs no bytes of the part (the part holds the paragraph's text, the
//! elements hold their own bytes):
//!
//! * **What an element is** ([`mark_name`], [`field_name`]): the local names the writer places. A name R10 does not name (a change mark, an index mark, a link,
//!   any other field) is no element of this module, and a paragraph that holds one is refused in words where the writer cannot keep it.
//! * **Where an element goes** ([`land`], [`Mapping`]): an element's offset in the model's text (as the reader counts) is carried over by the block's common
//!   prefix and suffix (item 3), to the paragraph the new text has there, or, when the new side holds no paragraph, to the start of the next paragraph the plan
//!   keeps, or the end of the last (item 3); a field is kept when its characters lie wholly in the prefix or the suffix (item 4). **A field the model holds as a field
//!   whose characters a refresh changed** keeps its element too, written again around the new characters where the model has them (the changed fields of a
//!   block and the model's fields no kept field lands on are paired in order, when there are as many of each; anything else is item 4's plain text and note).
//! * **How the characters round it are written** ([`Spaces`], item 6): a paragraph with elements is written in one pass over its characters, and a space is
//!   written as `text:s` where a plain one would not read back as one space: after another space with only elements between, at the start of the paragraph
//!   after elements alone, after a field, and at its end.
//!
//! The work is linear: each block's text is compared once, front and back, and each element is mapped and placed once.

use std::collections::HashMap;

use crate::read::readback::MOST_SPACES;

/// The words of a refusal for the memory the elements of a retext would hold.
pub(crate) const MEMORY_WORDS: &str = "memory for the zero-width elements of a retext";

/// The bytes of memory counted for each element a retext holds (the element, where it lands, and the note of what it is): charged to the memory a save may hold
/// before the elements are listed.
pub(crate) const ELEMENT_BYTES: u64 = 96;

/// The local name of a zero-width element R10 places (a point or the two ends of a range of bookmarks and of reference marks, and a soft page break), as the
/// writer knows it: `None` for any other.
pub(crate) fn mark_name(local: &str) -> Option<&'static str> {
    Some(match local {
        "bookmark" => "bookmark",
        "bookmark-start" => "bookmark-start",
        "bookmark-end" => "bookmark-end",
        "reference-mark" => "reference-mark",
        "reference-mark-start" => "reference-mark-start",
        "reference-mark-end" => "reference-mark-end",
        "soft-page-break" => "soft-page-break",
        _ => return None,
    })
}

/// The local names of the text fields a retext keeps whole (the fields of the standard that show their characters only: the model holds the shown text among the paragraph's characters, and
/// for the page number, the page count and the file name, which the reader maps to a `Field` row, also the `Fld` part and its row), in order.
const FIELDS: [&str; 45] = [
    "author-initials",
    "author-name",
    "chapter",
    "character-count",
    "creation-date",
    "creation-time",
    "date",
    "description",
    "editing-cycles",
    "editing-duration",
    "file-name",
    "image-count",
    "initial-creator",
    "keywords",
    "modification-date",
    "modification-time",
    "object-count",
    "page-count",
    "page-number",
    "paragraph-count",
    "print-date",
    "print-time",
    "printed-by",
    "sender-city",
    "sender-company",
    "sender-country",
    "sender-email",
    "sender-fax",
    "sender-firstname",
    "sender-initials",
    "sender-lastname",
    "sender-phone-private",
    "sender-phone-work",
    "sender-position",
    "sender-postal-code",
    "sender-state-or-province",
    "sender-street",
    "sender-title",
    "subject",
    "table-count",
    "template-name",
    "time",
    "title",
    "user-defined",
    "word-count",
];

/// The local name of a text field the model holds as plain text, as the writer knows it: `None` for any other element.
pub(crate) fn field_name(local: &str) -> Option<&'static str> {
    FIELDS.binary_search(&local).ok().and_then(|at| FIELDS.get(at)).copied()
}

/// A zero-width element or a kept field of the file: where its element is, where it stood in its paragraph, and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Zero {
    /// The whole element, from its `<` to after its `>` (or its end tag): copied byte for byte wherever it is put.
    pub bytes: (usize, usize),
    /// The offset of the character it stands before, in the paragraph's characters as the reader counts them (a mark between two spaces that collapse is
    /// after the space; one after the last character is at the paragraph's length). After a land: the offset in the paragraph it lands in.
    pub at: usize,
    /// For a field: how many characters it shows (what the model holds of it); 0 for a mark and for a field that shows none.
    pub len: usize,
    /// The local name of a field, as the note names it; `None` for a mark.
    pub field: Option<&'static str>,
    /// The namespace scope of the paragraph the element stood in: what a prefix in its bytes is bound to.
    pub scope: u32,
    /// The order of the element in the text of the shape (by paragraph, then by place): elements at one offset are written in this order.
    pub seq: usize,
    /// A soft page break that stood **between paragraphs** (R10 item 1: an anchor at the start of the paragraph after it): its `bytes` begin where the white space
    /// before it does, and when it lands on a paragraph's start it is written between the paragraphs, or at the start of the paragraph's list item (item 5); where it
    /// lands inside a paragraph, or at the end of one, it is written there as a mark is, from the element's own start.
    pub between: bool,
    /// For a field the model holds as a field (a `fld` and its row) whose characters a refresh changed: the characters it shows now. Its element is written again
    /// around them, its start and end tags as the file had them, and `len` is their count (a field never loses its element in a save). `None` for
    /// every other element, which is written as its bytes.
    pub shown: Option<String>,
}

/// Where an element goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Target {
    /// The paragraph of the model with this number, at the element's `at`.
    New(usize),
    /// The start of the paragraph of the file with this number, which the plan keeps (or edits in place): the paragraph's own bytes stay, and the element is put
    /// in at the start.
    Start(usize),
    /// The end of the paragraph of the file with this number.
    End(usize),
}

/// An element and where it lands.
#[derive(Clone, Debug)]
pub(crate) struct Landed {
    pub target: Target,
    pub zero: Zero,
}

/// What the check of the bytes made expects of the zero-width elements and fields of a paragraph a retext wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expect {
    /// A paragraph written whole or put in, by its number in the model: the elements it holds are these, each as the offset it stands at and its bytes, in
    /// order, and no other (an empty list is a paragraph that holds none).
    Exact { index: usize, items: Vec<(usize, String)> },
    /// A paragraph kept or edited in place: the elements put in at its start and at its end (the bytes of each, in order). Its own stay where they are.
    Edges { index: usize, start: Vec<String>, end: Vec<String> },
}

/// What a block of the plan is, as texts: the paragraphs of the file it takes out or rewrites, and those of the model it puts in or rewrites, each with its
/// number and its characters, and the paragraphs the plan keeps just before and just after it (by the file's number).
pub(crate) struct Block<'a> {
    pub old: &'a [(usize, &'a str)],
    pub new: &'a [(usize, &'a str)],
    /// The fields the model holds in each paragraph of `new`, in the same order (a paragraph past the end of the list holds none): see [`FieldAt`].
    pub new_fields: &'a [&'a [FieldAt]],
    pub before: Option<usize>,
    pub after: Option<usize>,
}

/// A field the model holds in a paragraph (a `fld`, whose shown characters are among the paragraph's): its first character's offset and how many characters it
/// shows, as the reader counts them, and the bytes of those characters in the paragraph's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldAt {
    pub at: usize,
    pub len: usize,
    pub bytes: (usize, usize),
}

/// The fields of the standard that the reader maps to a field of the model (a page number, a page count, a file name, and a date or a time
/// that is not fixed): only these can be the file's element of a field the model holds.
const MODEL_FIELDS: [&str; 5] = ["date", "file-name", "page-count", "page-number", "time"];

/// Whether characters a field shows can be written inside its element as plain text and read back as themselves: no white space that the collapsing of a
/// paragraph would change (a tab, a line feed, a space at either end or two in a row) and no character a part cannot hold.
fn plain_field_text(shown: &str) -> bool {
    !shown.is_empty()
        && !shown.starts_with(' ')
        && !shown.ends_with(' ')
        && !shown.contains("  ")
        && !shown.chars().any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
}

/// The characters of the paragraphs joined by U+000A, in order.
fn joined<'a>(parts: &'a [&'a str]) -> impl Iterator<Item = char> + 'a {
    // A line feed before each paragraph, the first of them left out.
    parts.iter().flat_map(|s| std::iter::once('\n').chain(s.chars())).skip(1)
}

/// The same, from the last character back.
fn joined_back<'a>(parts: &'a [&'a str]) -> impl Iterator<Item = char> + 'a {
    // A line feed after each paragraph's characters, the last of them (the one before the text's first character) one more than the text holds: the walk
    // of [`Mapping::of`] is bounded by the length of the shorter text, so it never reaches it.
    parts.iter().rev().flat_map(|s| s.chars().rev().chain(std::iter::once('\n')))
}

/// The carry-over of an offset of the old text of a block to the new text of it (R10 item 3): the longest common prefix, of length `p`, then the longest common
/// suffix of what is left, of length `s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mapping {
    pub p: usize,
    pub s: usize,
    pub old_len: usize,
    pub new_len: usize,
}

impl Mapping {
    /// The mapping of the text `old` to the text `new`, each the paragraphs joined by U+000A.
    pub(crate) fn of(old: &[&str], new: &[&str]) -> Mapping {
        let length = |parts: &[&str]| parts.iter().map(|s| s.chars().count()).fold(parts.len().saturating_sub(1), usize::saturating_add);
        let (old_len, new_len) = (length(old), length(new));
        let p = joined(old).zip(joined(new)).take_while(|(a, b)| a == b).count();
        let room = old_len.min(new_len).saturating_sub(p);
        let s = joined_back(old).zip(joined_back(new)).take(room).take_while(|(a, b)| a == b).count();
        Mapping { p, s, old_len, new_len }
    }

    /// Where the offset `x` of the old text is in the new: an offset up to `p` stays, one from `old_len - s` on moves by the change of length, any other is `p`.
    pub(crate) fn map(&self, x: usize) -> usize {
        if x <= self.p {
            x
        } else if x >= self.old_len.saturating_sub(self.s) {
            // x + new_len - old_len, which is at least new_len - s, never under 0.
            x.saturating_add(self.new_len).saturating_sub(self.old_len).min(self.new_len)
        } else {
            self.p
        }
    }

    /// Whether the characters from `x` for `len` lie wholly in the common prefix, or wholly in the common suffix: where each moves (a field is kept only then).
    pub(crate) fn kept(&self, x: usize, len: usize) -> Option<usize> {
        if x.saturating_add(len) <= self.p {
            Some(x)
        } else if x >= self.old_len.saturating_sub(self.s) {
            // By the change of length, even at the one offset where the prefix and the suffix meet (which [`Mapping::map`] leaves to the prefix): the field's
            // characters are the suffix's.
            Some(x.saturating_add(self.new_len).saturating_sub(self.old_len))
        } else {
            None
        }
    }
}

/// The words of a block whose new side holds no paragraph and with no paragraph the plan keeps to take its elements.
const WORDS_NOWHERE: &str = "a text edit that takes every paragraph out and keeps none, with a zero-width element in one of them";

/// Carries the elements of the old paragraphs of one block over to where the new text has them (R10 item 3 and 4), adding them to `landed` in the order they stood
/// in, and the names of the fields whose characters changed (written as the new characters, without the element) to `plain`. `steps` counts the characters
/// compared. `Err` in words for what is not placed.
pub(crate) fn land(
    block: &Block<'_>,
    anchors: &HashMap<usize, Vec<Zero>>,
    steps: &mut u64,
    landed: &mut Vec<Landed>,
    plain: &mut Vec<&'static str>,
) -> Result<(), &'static str> {
    let old: Vec<&str> = block.old.iter().map(|(_, s)| *s).collect();
    let new: Vec<&str> = block.new.iter().map(|(_, s)| *s).collect();
    let map = Mapping::of(&old, &new);
    *steps = steps.saturating_add(map.old_len as u64).saturating_add(map.new_len as u64);
    let starts = |parts: &[&str]| -> Result<Vec<(usize, usize)>, &'static str> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        out.try_reserve(parts.len()).map_err(|_| MEMORY_WORDS)?;
        let mut at = 0usize;
        for s in parts {
            let len = s.chars().count();
            out.push((at, len));
            at = at.saturating_add(len).saturating_add(1);
        }
        Ok(out)
    };
    let (old_at, new_at) = (starts(&old)?, starts(&new)?);
    let paired = pair_refreshed(block, anchors, &map, (&old_at, &new_at), steps)?;
    let mut changed = 0usize;
    for ((index, _), (start, len)) in block.old.iter().zip(&old_at) {
        let Some(zeros) = anchors.get(index) else { continue };
        for z in zeros {
            *steps = steps.saturating_add(1);
            let x = start.saturating_add(z.at.min(*len));
            let y = if z.field.is_some() && z.len > 0 {
                match map.kept(x, z.len) {
                    Some(y) => y,
                    None => {
                        let mine = changed;
                        changed = changed.saturating_add(1);
                        // A field the model holds still, whose characters a refresh changed: its element is written again around the new characters, where the
                        // model has them.
                        if let Some(Some((paragraph, field))) = paired.as_ref().map(|p| p.get(mine).copied())
                            && let (Some((target, _)), Some(text)) =
                                (block.new.get(paragraph), block.new.get(paragraph).and_then(|(_, t)| t.get(field.bytes.0..field.bytes.1)))
                        {
                            landed.try_reserve(1).map_err(|_| MEMORY_WORDS)?;
                            let mut zero = z.clone();
                            zero.at = field.at;
                            zero.len = field.len;
                            zero.shown = Some(text.to_owned());
                            landed.push(Landed { target: Target::New(*target), zero });
                            continue;
                        }
                        // The characters changed: they stay as the new characters and the element goes, with a note (unless the text went with its paragraph).
                        if let Some(name) = z.field.filter(|_| !block.new.is_empty()) {
                            plain.try_reserve(1).map_err(|_| MEMORY_WORDS)?;
                            plain.push(name);
                        }
                        continue;
                    }
                }
            } else {
                map.map(x)
            };
            landed.try_reserve(1).map_err(|_| MEMORY_WORDS)?;
            let mut zero = z.clone();
            if block.new.is_empty() {
                let target = match (block.after, block.before) {
                    (Some(after), _) => Target::Start(after),
                    (None, Some(before)) => Target::End(before),
                    (None, None) => return Err(WORDS_NOWHERE),
                };
                zero.at = 0;
                landed.push(Landed { target, zero });
            } else {
                let at = new_at.partition_point(|(s, _)| *s <= y).saturating_sub(1);
                let ((index, _), (s, _)) = block.new.get(at).zip(new_at.get(at)).ok_or(WORDS_NOWHERE)?;
                zero.at = y.saturating_sub(*s);
                landed.push(Landed { target: Target::New(*index), zero });
            }
        }
    }
    Ok(())
}

/// Where a paragraph of a block starts in the block's text, and how many characters it has.
type Start = (usize, usize);

/// The fields of the file whose characters the block changed, each with the field of the model it is (the paragraph's place in `block.new` and the field), or
/// `None` when they are not the model's fields one for one. The model's fields of the new side that a field the mapping keeps lands on are that field's; the others
/// are taken by the changed fields in order, and only when there are as many of each and every changed field is of a kind the model holds as a field
/// ([`MODEL_FIELDS`]) and shows characters a field can hold as text ([`plain_field_text`]): that is a refresh, which changes a field's characters and nothing
/// around it. Anything else (a field taken out, another put in, a retyped fixed field) is written as R10 item 4 says.
fn pair_refreshed(
    block: &Block<'_>,
    anchors: &HashMap<usize, Vec<Zero>>,
    map: &Mapping,
    (old_at, new_at): (&[Start], &[Start]),
    steps: &mut u64,
) -> Result<Option<Vec<(usize, FieldAt)>>, &'static str> {
    if block.new_fields.iter().all(|f| f.is_empty()) {
        return Ok(None);
    }
    // Where each kept field lands in the new text, and whether every changed field is of a kind the model holds.
    let mut kept_at: Vec<usize> = Vec::new();
    let mut changed = 0usize;
    for ((index, _), (start, len)) in block.old.iter().zip(old_at) {
        let Some(zeros) = anchors.get(index) else { continue };
        for z in zeros.iter().filter(|z| z.field.is_some() && z.len > 0) {
            *steps = steps.saturating_add(1);
            let x = start.saturating_add(z.at.min(*len));
            match map.kept(x, z.len) {
                Some(y) => {
                    kept_at.try_reserve(1).map_err(|_| MEMORY_WORDS)?;
                    kept_at.push(y);
                }
                None if z.field.is_some_and(|name| MODEL_FIELDS.contains(&name)) => changed = changed.saturating_add(1),
                None => return Ok(None),
            }
        }
    }
    if changed == 0 {
        return Ok(None);
    }
    kept_at.sort_unstable();
    let mut free: Vec<(usize, FieldAt)> = Vec::new();
    for (k, ((_, text), (start, _))) in block.new.iter().zip(new_at).enumerate() {
        for field in block.new_fields.get(k).copied().unwrap_or_default() {
            *steps = steps.saturating_add(1);
            if kept_at.binary_search(&start.saturating_add(field.at)).is_ok() {
                continue;
            }
            if !text.get(field.bytes.0..field.bytes.1).is_some_and(plain_field_text) {
                return Ok(None);
            }
            free.try_reserve(1).map_err(|_| MEMORY_WORDS)?;
            free.push((k, *field));
        }
    }
    Ok((free.len() == changed).then_some(free))
}

/// Puts the elements that land in one paragraph in order and apart from what they cover (R10 item 4 and 5): by offset, in the order they stood in where they are at
/// one, and none inside the characters of a field before it (an element that maps inside a field's characters goes to the field's end). Returns the first offset
/// that does not lie within the paragraph (`total`), if there is one.
pub(crate) fn arrange(zeros: &mut [Zero], total: usize) -> Option<usize> {
    zeros.sort_by_key(|z| (z.at, z.seq));
    let mut floor = 0usize;
    let mut past = None;
    for z in zeros.iter_mut() {
        z.at = z.at.max(floor);
        if z.at.saturating_add(z.len) > total {
            past = Some(z.at);
        }
        floor = z.at.saturating_add(z.len);
    }
    past
}

/// What stands just before the next character as the writer writes a paragraph's content (the reader's white space state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    /// Nothing that holds a character: white space here is the paragraph's leading space.
    Start,
    /// A character, a tab, a line break.
    Char,
    /// A space, written plain or as `text:s`: a plain one after it would collapse into it.
    Space,
    /// A field's element.
    Field,
}

/// The writer of the characters of a paragraph that holds elements, in one pass over its characters in pieces: [`Spaces::text`] for each piece of characters and
/// [`Spaces::field`] for the characters a field element stands for. A first space is written as itself, a space is written as `text:s` where a plain one would not
/// read back as the one space (R10 item 6), and a row of spaces is written as one `text:s` with `text:c` when it starts with one.
pub(crate) struct Spaces<'a> {
    prefix: &'a str,
    declare: &'a str,
    last: Last,
    /// How many characters were written, and where the spaces at the paragraph's end start.
    at: usize,
    trailing_from: usize,
}

impl<'a> Spaces<'a> {
    /// A writer of the paragraph whose characters are `chars`, its elements under the prefix `prefix` and each carrying `declare`.
    pub(crate) fn new(chars: &str, prefix: &'a str, declare: &'a str) -> Spaces<'a> {
        let (mut total, mut spaces) = (0usize, 0usize);
        for c in chars.chars().rev() {
            total += 1;
            if c == ' ' && total == spaces + 1 {
                spaces += 1;
            }
        }
        Spaces { prefix, declare, last: Last::Start, at: 0, trailing_from: total - spaces }
    }

    fn spaces(&self, n: usize, out: &mut String) {
        let (p, declare) = (self.prefix, self.declare);
        let mut left = n;
        while left > 0 {
            let now = left.min(usize::try_from(MOST_SPACES).unwrap_or(65_536));
            left -= now;
            if now == 1 {
                out.push_str(&format!("<{p}:s{declare}/>"));
            } else {
                out.push_str(&format!("<{p}:s{declare} {p}:c=\"{now}\"/>"));
            }
        }
    }

    /// Writes a piece of characters.
    pub(crate) fn text(&mut self, piece: &str, out: &mut String) {
        let (p, declare) = (self.prefix, self.declare);
        let mut plain = String::new();
        let flush = |plain: &mut String, out: &mut String| {
            out.push_str(&super::text::escape_text(plain));
            plain.clear();
        };
        let mut it = piece.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                ' ' => {
                    let mut n = 1usize;
                    while it.peek() == Some(&' ') {
                        it.next();
                        n += 1;
                    }
                    let literal = self.at >= self.trailing_from || self.last != Last::Char;
                    self.at += n;
                    self.last = Last::Space;
                    if literal {
                        flush(&mut plain, out);
                        self.spaces(n, out);
                    } else {
                        plain.push(' ');
                        flush(&mut plain, out);
                        self.spaces(n - 1, out);
                    }
                }
                '\t' => {
                    flush(&mut plain, out);
                    out.push_str(&format!("<{p}:tab{declare}/>"));
                    self.at += 1;
                    self.last = Last::Char;
                }
                '\u{2028}' => {
                    flush(&mut plain, out);
                    out.push_str(&format!("<{p}:line-break{declare}/>"));
                    self.at += 1;
                    self.last = Last::Char;
                }
                other => {
                    plain.push(other);
                    self.at += 1;
                    self.last = Last::Char;
                }
            }
        }
        flush(&mut plain, out);
    }

    /// Notes that a field's element was written, standing for `len` characters of the paragraph.
    pub(crate) fn field(&mut self, len: usize) {
        self.at = self.at.saturating_add(len);
        self.last = Last::Field;
    }

    /// Notes that an element was written that shows `len` characters **the last of which is no white space** (the model writer's fields, [`crate::write::text::model`]): a space after it
    /// is a space the reader reads, as after a character, and is written plain.
    pub(crate) fn shown(&mut self, len: usize) {
        self.at = self.at.saturating_add(len);
        self.last = Last::Char;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero(at: usize, len: usize, field: Option<&'static str>, seq: usize) -> Zero {
        Zero { bytes: (seq * 10, seq * 10 + 5), at, len, field, scope: 0, seq, between: false, shown: None }
    }

    #[test]
    fn the_fields_are_listed_in_order_once_and_each_is_found() {
        assert!(FIELDS.windows(2).all(|w| w[0] < w[1]), "sorted and with no name twice: the search is a binary one");
        for name in FIELDS {
            assert_eq!(field_name(name), Some(name), "{name}");
            assert_eq!(mark_name(name), None, "{name}");
        }
        // The names of the neighbours in the order, and of the ends, are not fields.
        for name in ["author", "zzz", "aaa", "date-", "tim", "titles", "sender", "word", "page", "file"] {
            assert_eq!(field_name(name), None, "{name}");
        }
        // The count is the list's (a name added to one and not the other would not compile).
        assert_eq!(FIELDS.len(), 45);
    }

    #[test]
    fn the_names_places_are_the_marks_and_the_fields_and_nothing_else() {
        for name in ["bookmark", "bookmark-start", "bookmark-end", "reference-mark", "reference-mark-start", "reference-mark-end", "soft-page-break"]
        {
            assert_eq!(mark_name(name), Some(name));
            assert_eq!(field_name(name), None, "a mark is no field");
        }
        for name in ["author-name", "title", "date", "time", "page-number", "page-count", "file-name"] {
            assert_eq!(field_name(name), Some(name));
            assert_eq!(mark_name(name), None);
        }
        // What R10 does not name is neither.
        for name in ["a", "span", "s", "tab", "line-break", "change", "change-start", "toc-mark", "alphabetical-index-mark", ""] {
            assert_eq!((mark_name(name), field_name(name)), (None, None), "{name}");
        }
    }

    /// `alpha beta gamma` (16) to `alpha gamma` (11); p is 6 and s is 5; an offset in the removed `beta ` goes to p.
    #[test]
    fn the_prefix_and_the_suffix_and_an_offset_in_the_removed_text() {
        let m = Mapping::of(&["alpha beta gamma"], &["alpha gamma"]);
        assert_eq!(m, Mapping { p: 6, s: 5, old_len: 16, new_len: 11 });
        assert_eq!(m.map(10), 6, "inside the removed text");
        assert_eq!(m.map(0), 0);
        assert_eq!(m.map(6), 6, "x up to p stays");
        assert_eq!(m.map(11), 6, "the start of the suffix is the end of the removed text, mapped to p");
        assert_eq!(m.map(16), 11, "the end moves by the change of length");
    }

    /// `one two four` (12) to `one four` (8): both ends of a range inside the removed text map to 4, and keep their order.
    #[test]
    fn a_range_that_collapses_maps_both_ends_to_one_place() {
        let m = Mapping::of(&["one two four"], &["one four"]);
        assert_eq!(m, Mapping { p: 4, s: 4, old_len: 12, new_len: 8 });
        assert_eq!((m.map(4), m.map(7)), (4, 4));
    }

    /// The prefix first. `one two three` to `one three` is p = 5 (`one t`) and s = 4 (`hree`), and 7 maps to 5; the suffix first would give s = 6 and 3.
    #[test]
    fn the_prefix_is_taken_first() {
        let m = Mapping::of(&["one two three"], &["one three"]);
        assert_eq!(m, Mapping { p: 5, s: 4, old_len: 13, new_len: 9 });
        assert_eq!(m.map(7), 5);
        assert_ne!(m.map(7), 3, "taking the suffix first (s = 6) puts it at 3");
    }

    #[test]
    fn a_suffix_can_reach_across_the_line_feed_between_paragraphs() {
        // `x\nab` to `y\nab`: the suffix is `\nab`, the line feed with it.
        assert_eq!(Mapping::of(&["x", "ab"], &["y", "ab"]), Mapping { p: 0, s: 3, old_len: 4, new_len: 4 });
        assert_eq!(Mapping::of(&["ab", "x"], &["ab", "y"]), Mapping { p: 3, s: 0, old_len: 4, new_len: 4 });
        assert_eq!(Mapping::of(&["a", "b", "c"], &["a", "b", "c", "d"]), Mapping { p: 5, s: 0, old_len: 5, new_len: 7 });
        assert_eq!(Mapping::of(&["b", "c"], &["a", "b", "c"]), Mapping { p: 0, s: 3, old_len: 3, new_len: 5 });
    }

    #[test]
    fn the_suffix_never_overlaps_the_prefix_and_blocks_of_several_paragraphs_are_joined_by_a_line_feed() {
        // `aa` to `aaa`: the prefix takes the two, the suffix has nothing left to match.
        assert_eq!(Mapping::of(&["aa"], &["aaa"]), Mapping { p: 2, s: 0, old_len: 2, new_len: 3 });
        assert_eq!(Mapping::of(&["aaa"], &["aa"]), Mapping { p: 2, s: 0, old_len: 3, new_len: 2 });
        // Two paragraphs, `ab\ncd`, to one, `abcd`: the line feed is what differs.
        let m = Mapping::of(&["ab", "cd"], &["abcd"]);
        assert_eq!(m, Mapping { p: 2, s: 2, old_len: 5, new_len: 4 });
        // Nothing on either side.
        assert_eq!(Mapping::of(&[], &[]), Mapping { p: 0, s: 0, old_len: 0, new_len: 0 });
        assert_eq!(Mapping::of(&["x"], &[]), Mapping { p: 0, s: 0, old_len: 1, new_len: 0 });
        assert_eq!(Mapping::of(&["x"], &[]).map(1), 0);
        // Characters of more than one byte count once.
        assert_eq!(Mapping::of(&["ée"], &["éée"]), Mapping { p: 1, s: 1, old_len: 2, new_len: 3 });
    }

    #[test]
    fn a_field_is_kept_when_its_characters_lie_wholly_in_the_prefix_or_the_suffix() {
        // `by Ann today` to `by Bob today`: p = 3, s = 6 (` today`), the field `Ann` is at 3 to 6.
        let m = Mapping::of(&["by Ann today"], &["by Bob today"]);
        assert_eq!((m.p, m.s), (3, 6));
        assert_eq!(m.kept(3, 3), None, "the characters changed");
        // `today` made bold, nothing else changed: the whole text is the prefix.
        let same = Mapping::of(&["by Ann today"], &["by Ann today"]);
        assert_eq!(same.kept(3, 3), Some(3));
        // A field after an insertion moves with the suffix; one before it stays.
        let m = Mapping::of(&["Ann x"], &["Ann new x"]);
        assert_eq!(m.kept(0, 3), Some(0));
        let m = Mapping::of(&["x Ann"], &["new x Ann"]);
        assert_eq!(m.kept(2, 3), Some(6));
        // At the one offset where the prefix and the suffix meet, a mark stays (R10 item 3: x up to p) and a field in the suffix moves.
        let m = Mapping::of(&["by Ann today"], &["by new Ann today"]);
        assert_eq!((m.p, m.s), (3, 9));
        assert_eq!(m.map(3), 3);
        assert_eq!(m.kept(3, 3), Some(7));
    }

    fn land_one(
        old: &[&str],
        new: &[&str],
        zeros: Vec<(usize, Zero)>,
        before: Option<usize>,
        after: Option<usize>,
    ) -> (Vec<Landed>, Vec<&'static str>) {
        let old_paras: Vec<(usize, &str)> = old.iter().enumerate().map(|(i, s)| (i, *s)).collect();
        let new_paras: Vec<(usize, &str)> = new.iter().enumerate().map(|(i, s)| (i, *s)).collect();
        let mut anchors: HashMap<usize, Vec<Zero>> = HashMap::new();
        for (p, z) in zeros {
            anchors.entry(p).or_default().push(z);
        }
        let (mut landed, mut plain) = (Vec::new(), Vec::new());
        let block = Block { old: &old_paras, new: &new_paras, new_fields: &[], before, after };
        land(&block, &anchors, &mut 0, &mut landed, &mut plain).unwrap();
        (landed, plain)
    }

    #[test]
    fn an_element_lands_in_the_paragraph_the_new_text_has_at_its_place_and_at_the_end_of_one_it_stays_there() {
        // `ab\ncd` to `ab\nXcd`; a mark at the start of the second paragraph (offset 3 of the block) stays before its `c`... the prefix is `ab\n`.
        let (landed, _) = land_one(&["ab", "cd"], &["ab", "Xcd"], vec![(1, zero(0, 0, None, 0))], None, None);
        assert_eq!(landed.len(), 1);
        assert_eq!((landed[0].target, landed[0].zero.at), (Target::New(1), 0));
        // A mark at the end of the first paragraph (offset 2): before the line feed, so the end of the first paragraph.
        let (landed, _) = land_one(&["ab", "cd"], &["ab", "Xcd"], vec![(0, zero(2, 0, None, 0))], None, None);
        assert_eq!((landed[0].target, landed[0].zero.at), (Target::New(0), 2));
    }

    #[test]
    fn a_block_with_no_new_paragraph_sends_its_elements_to_the_start_of_the_next_paragraph_kept_or_the_end_of_the_last() {
        // The removed paragraph's start anchor goes to the start of the next paragraph kept.
        let (landed, _) = land_one(&["start here"], &[], vec![(0, zero(6, 0, None, 0))], Some(3), Some(5));
        assert_eq!((landed[0].target, landed[0].zero.at), (Target::Start(5), 0));
        // None follows: the end of the last paragraph the plan kept.
        let (landed, _) = land_one(&["start here"], &[], vec![(0, zero(6, 0, None, 0))], Some(3), None);
        assert_eq!(landed[0].target, Target::End(3));
        // Neither: refused in words.
        let old = [(0usize, "x")];
        let mut anchors = HashMap::new();
        anchors.insert(0usize, vec![zero(0, 0, None, 0)]);
        let block = Block { old: &old, new: &[], new_fields: &[], before: None, after: None };
        assert!(land(&block, &anchors, &mut 0, &mut Vec::new(), &mut Vec::new()).is_err());
    }

    /// A field that shows nothing is a point like a mark: it goes where the mapping puts a point, and is never dropped for its characters.
    #[test]
    fn a_field_that_shows_nothing_is_carried_as_a_point() {
        let empty = zero(2, 0, Some("title"), 0);
        // `abcd` to `ad`: p = 1, s = 1; the point at 2 is in the removed text and goes to p, as a mark does.
        let (landed, plain) = land_one(&["abcd"], &["ad"], vec![(0, empty.clone())], None, None);
        assert_eq!((landed.len(), plain.len()), (1, 0), "{plain:?}");
        assert_eq!((landed[0].target, landed[0].zero.at, landed[0].zero.len), (Target::New(0), 1, 0));
        // One in the prefix stays, one in the suffix moves by the change of length.
        let (landed, _) = land_one(&["abcd"], &["abXcd"], vec![(0, zero(1, 0, Some("title"), 0)), (0, zero(3, 0, Some("title"), 1))], None, None);
        assert_eq!(landed.iter().map(|l| l.zero.at).collect::<Vec<_>>(), [1, 4]);
    }

    /// `page 3 of 9` to `page 12 of 9`, the page number a field of the model before and after (a refresh). The page count lies in the suffix and is
    /// kept; the page number's characters changed, and it lands on the model's field with the new characters. Without the model's field (or with a field of a kind
    /// the model holds as text) it is item 4's plain text and note.
    #[test]
    fn a_field_a_refresh_changed_lands_on_the_models_field_with_its_new_characters() {
        let old = [(0usize, "page 3 of 9")];
        let new = [(0usize, "page 12 of 9")];
        let mut anchors: HashMap<usize, Vec<Zero>> = HashMap::new();
        anchors.insert(0, vec![zero(5, 1, Some("page-number"), 0), zero(10, 1, Some("page-count"), 1)]);
        let both = [FieldAt { at: 5, len: 2, bytes: (5, 7) }, FieldAt { at: 11, len: 1, bytes: (11, 12) }];
        let fields: [&[FieldAt]; 1] = [&both];
        let block = Block { old: &old, new: &new, new_fields: &fields, before: None, after: None };
        let (mut landed, mut plain) = (Vec::new(), Vec::new());
        land(&block, &anchors, &mut 0, &mut landed, &mut plain).unwrap();
        assert!(plain.is_empty(), "{plain:?}");
        let got: Vec<(usize, usize, Option<&str>)> = landed.iter().map(|l| (l.zero.at, l.zero.len, l.zero.shown.as_deref())).collect();
        assert_eq!(got, [(5, 2, Some("12")), (11, 1, None)]);
        // The model has the page count only: the changed field has no field to be, and is the new characters with a note.
        let count_only = [FieldAt { at: 11, len: 1, bytes: (11, 12) }];
        let fields: [&[FieldAt]; 1] = [&count_only];
        let block = Block { old: &old, new: &new, new_fields: &fields, before: None, after: None };
        let (mut landed, mut plain) = (Vec::new(), Vec::new());
        land(&block, &anchors, &mut 0, &mut landed, &mut plain).unwrap();
        assert_eq!(plain, ["page-number"]);
        assert_eq!(landed.len(), 1);
        // A field the model holds as text (an author's name) whose characters changed pairs with nothing, and nor does any other changed field of the block.
        let mut anchors: HashMap<usize, Vec<Zero>> = HashMap::new();
        anchors.insert(0, vec![zero(5, 1, Some("author-name"), 0), zero(10, 1, Some("page-count"), 1)]);
        let fields: [&[FieldAt]; 1] = [&both];
        let block = Block { old: &old, new: &new, new_fields: &fields, before: None, after: None };
        let (mut landed, mut plain) = (Vec::new(), Vec::new());
        land(&block, &anchors, &mut 0, &mut landed, &mut plain).unwrap();
        assert_eq!(plain, ["author-name"]);
        // Characters a field cannot hold as plain text (two spaces in a row) are written as R10 item 4 says.
        let spaced = [(0usize, "page 1  2 of 9")];
        let wide = [FieldAt { at: 5, len: 4, bytes: (5, 9) }, FieldAt { at: 13, len: 1, bytes: (13, 14) }];
        let fields: [&[FieldAt]; 1] = [&wide];
        let mut anchors: HashMap<usize, Vec<Zero>> = HashMap::new();
        anchors.insert(0, vec![zero(5, 1, Some("page-number"), 0), zero(10, 1, Some("page-count"), 1)]);
        let block = Block { old: &old, new: &spaced, new_fields: &fields, before: None, after: None };
        let (mut landed, mut plain) = (Vec::new(), Vec::new());
        land(&block, &anchors, &mut 0, &mut landed, &mut plain).unwrap();
        assert_eq!(plain, ["page-number"]);
    }

    #[test]
    fn a_field_whose_characters_changed_is_named_for_the_note_and_does_not_land_unless_the_text_went_with_its_paragraph() {
        // `by Ann today` to `by Bob today`: the field is dropped and named.
        let field = zero(3, 3, Some("author-name"), 0);
        let (landed, plain) = land_one(&["by Ann today"], &["by Bob today"], vec![(0, field.clone())], None, None);
        assert!(landed.is_empty());
        assert_eq!(plain, ["author-name"]);
        // Unchanged characters: kept, and lands where it was.
        let (landed, plain) = land_one(&["by Ann today"], &["by Ann today"], vec![(0, field.clone())], None, None);
        assert_eq!((landed.len(), plain.len()), (1, 0));
        assert_eq!((landed[0].target, landed[0].zero.at, landed[0].zero.len), (Target::New(0), 3, 3));
        // The paragraph went: no characters are written, no note.
        let (landed, plain) = land_one(&["by Ann today"], &[], vec![(0, field)], Some(1), Some(2));
        assert!(landed.is_empty() && plain.is_empty());
    }

    #[test]
    fn elements_land_in_the_order_they_stood_in_and_arrange_keeps_them_apart_from_a_field_s_characters() {
        let mut zs = vec![zero(4, 0, None, 2), zero(2, 0, None, 1), zero(4, 0, None, 0), zero(3, 3, Some("title"), 3)];
        assert_eq!(arrange(&mut zs, 10), None);
        let order: Vec<(usize, usize)> = zs.iter().map(|z| (z.at, z.seq)).collect();
        // By offset, then by seq; the field at 3 covers 3 to 6, so the elements at 4 (seq 0 and 2) go to 6... they were sorted before the field? 2, 3(field), then 4s.
        assert_eq!(order, [(2, 1), (3, 3), (6, 0), (6, 2)]);
        // A field that does not fit in the paragraph is reported.
        let mut zs = vec![zero(8, 5, Some("title"), 0)];
        assert_eq!(arrange(&mut zs, 10), Some(8));
    }

    /// What the writer makes of the paragraph `chars` cut into `pieces` (the stretches between elements and runs: no element is written, they write nothing of
    /// their own here), a field of this many characters standing after the piece with this number.
    fn written(chars: &str, pieces: &[&str], fields: &[(usize, usize)]) -> String {
        let mut w = Spaces::new(chars, "t", "");
        let mut out = String::new();
        for (k, piece) in pieces.iter().enumerate() {
            w.text(piece, &mut out);
            for (_, len) in fields.iter().filter(|(after, _)| *after == k) {
                out.push_str("[F]");
                w.field(*len);
            }
        }
        out
    }

    #[test]
    fn spaces_are_written_as_a_whole_paragraph_s_are_when_nothing_is_between() {
        // One piece alone is the encoding of a whole paragraph.
        for s in ["a b", "a  b", "a   b", "  a", "a  ", " ", "a  b  c   d", "  a  b  ", "a\tb", "a \tb", "x\u{2028} y"] {
            assert_eq!(written(s, &[s], &[]), super::super::text::encode(s, "t", ""), "{s:?}");
        }
    }

    /// `a b` cut after the space (a mark between the pieces): the space stays plain, as it is not at the paragraph's end and the character after it is not a
    /// space; `a  b` cut after the first space: the second follows a space, so it is `text:s`.
    #[test]
    fn a_space_after_a_space_with_a_mark_between_is_text_s_and_the_first_stays_plain() {
        assert_eq!(written("a b", &["a ", "b"], &[]), "a b");
        assert_eq!(written("a  b", &["a ", " b"], &[]), "a <t:s/>b");
        assert_eq!(written("a  b", &["a  b"], &[]), "a <t:s/>b", "a row that stays whole is as before");
        // Cut before the first space, and between the three of a row: only the first of a row is plain, wherever the row is cut.
        assert_eq!(written("a  b", &["a", "  b"], &[]), "a <t:s/>b");
        assert_eq!(written("a   b", &["a ", " ", " b"], &[]), "a <t:s/><t:s/>b");
    }

    #[test]
    fn a_space_at_the_start_of_the_paragraph_after_marks_alone_is_text_s() {
        // The paragraph's first character is a space: `text:s`, however many elements come before it (they write nothing here).
        assert_eq!(written(" a", &["", " a"], &[]), "<t:s/>a");
        assert_eq!(written("  a", &["", " ", " a"], &[]), "<t:s/><t:s/>a");
    }

    /// `by Ann today` with the field `Ann` of 3 characters: `by ` (plain: a space before a field is read back as it was), the field, then ` today`, whose first
    /// space follows a field and is `text:s`, so that both forms of the collapsing algorithm read it.
    #[test]
    fn a_space_after_a_field_is_text_s_and_one_before_it_is_plain() {
        assert_eq!(written("by Ann today", &["by ", " today"], &[(0, 3)]), "by [F]<t:s/>today");
        // After a field, a row of spaces is one `text:s` with its count; after a field that shows nothing too.
        assert_eq!(written("x   y", &["x", "   y"], &[(0, 0)]), "x[F]<t:s t:c=\"3\"/>y");
        // A field alone, with no space beside it, changes nothing.
        assert_eq!(written("abc", &["a", "c"], &[(0, 1)]), "a[F]c");
    }

    #[test]
    fn the_spaces_at_the_end_of_the_paragraph_are_text_s_however_the_pieces_are_cut() {
        // `a` and two spaces, cut after the first space: both are at the end.
        assert_eq!(written("a  ", &["a ", " "], &[]), "a<t:s/><t:s/>");
        // A counted row longer than the reader's bound is written as several elements.
        let long = format!("a{}", " ".repeat(65_536 + 3));
        let out = written(&long, &[&long], &[]);
        assert_eq!(out.matches("<t:s ").count(), 2, "{}", out.len());
        assert!(out.contains("t:c=\"65536\"") && out.contains("t:c=\"3\""));
    }

    #[test]
    fn a_tab_or_a_line_break_moves_the_place_the_spaces_at_the_end_are_told_by() {
        // The spaces after a tab or a line break at the paragraph's end are at the end: counted.
        assert_eq!(written("a\t ", &["a\t ", ""], &[]), "a<t:tab/><t:s/>");
        assert_eq!(written("a\u{2028}  ", &["a\u{2028}  "], &[]), "a<t:line-break/><t:s t:c=\"2\"/>");
        // And not at the end: a plain space (the characters after it).
        assert_eq!(written("a\t b", &["a\t b"], &[]), "a<t:tab/> b");
        assert_eq!(written("a\u{2028} b c ", &["a\u{2028} b c "], &[]), "a<t:line-break/> b c<t:s/>");
    }

    #[test]
    fn a_tab_a_line_break_and_text_that_needs_escaping_are_written_as_writes_them() {
        assert_eq!(written("a\tb\u{2028}c", &["a\tb\u{2028}c"], &[]), "a<t:tab/>b<t:line-break/>c");
        assert_eq!(written("<a&b>", &["<a&b>"], &[]), "&lt;a&amp;b&gt;");
        // A space after a tab is not after a space: plain.
        assert_eq!(written("a\t b", &["a\t b"], &[]), "a<t:tab/> b");
    }
}
