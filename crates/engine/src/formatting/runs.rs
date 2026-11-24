//! Splitting the runs and paragraphs of a text at the ends of a range, so that formatting reaches the range and nothing else.
//!
//! A shape's text is kept in parts: the characters, the `cp` and `pp` markers that start a run and a paragraph, `tp`
//! markers, and `fld` fields. A marker names a row of `Character` or `Paragraph` by `IX`, and holds until the next of its kind. To format the
//! characters `from..to` without formatting their neighbours, the run (or the paragraph) that starts before `from` or goes on past `to` is cut
//! there, and what is inside the range gets a row of its own.
//!
//! ours; no specification says what a person's formatting of a part of a run does to the file:
//!
//! * A **new row** holds **a copy of the cells the run's row resolves to** (the formulas as written and the values saved), with the change.
//!   [`plan`] gives each new marker the next `IX` above every row the shape's text and its sheet, master and styles hold (so it inherits nothing by
//!   the key it has); the numbers a file ends up with are decided by the caller (`text.rs`) in one of two cases, both of which write **every row
//!   the text names as a local row** when a split is made (a reader that takes the first local row for run 0, as LibreOffice does, then reads a
//!   text whose row 0 a style supplies right):
//!   * **Text order** (rows are numbered in text order, "a writer numbers them this way"): when the text's first marker names row 0 and
//!     **no master or style holds a row at `IX` 1 or above** in the section (the counts: a `cp` finds its row on a master in 935 of about
//!     125,000 runs and in a style in 344, so this is about 99 in 100 texts), the **rows** are numbered 0, 1, 2 in the order they first appear in
//!     the text and each marker takes the number of its row (rows are renumbered, not markers: markers that name one row, which a reader follows,
//!     go on naming one, so a text of a million markers that name one row writes one row), each row takes the cells of the run it holds
//!     (rows that move are written whole, and rows left past the last number are taken out, all of a section's in one pass). A row that stays
//!     where it is and is not in the range is not touched.
//!   * **Next free**: otherwise a row of a master or a style at `IX` 1 or above may be named by a marker, and a row that held another run's cells
//!     would inherit those cells by its key, so no row is moved: each new marker keeps the next `IX` above every row there is, which is text
//!     order only when the split is in the last run (a reader that follows the `IX` as written, reads it right whatever the order; one
//!     that reads rows by position, as LibreOffice does, does not).
//!   * **No more rows than a section numbers**: a text that names more than [`MAX_NAMED_ROWS`] rows (the numbers 0 to 65,535, all a reader takes)
//!     is refused with the words of a full section where a command would have to write them (a split needs a new number, and there is none
//!     above the highest, so neither case can take it; a format of the whole text writes a row for each one it names).
//! * The part of the run before the range keeps the row and the marker it had. The part inside the range gets a new marker and a new row (a copy,
//!   changed). The part after the range gets a new marker and a new row (a copy, unchanged). A part of a run that is the whole of it keeps its row,
//!   which is changed in place, and no marker is added.
//! * A row that a run in the range shares with a run outside it (two markers naming one row, which a reader follows) is not changed for
//!   both: the run in the range is given a copy.
//! * A text with no marker of the kind has row 0 from its first character; a marker for row 0 is written first when a split needs one.
//! * **Where a marker goes.** A run starts at the first character of the range, after the markers and fields that stand between it and the
//!   characters before (a field there stays with the run before). A run starts after the range's last character before the fields and markers that
//!   follow it, unless a marker is there already. A paragraph starts right after the line break that ends the one before, so a field after the
//!   break is the new paragraph's. A field at the edge of a range is in the run it is in, whatever the range says.
//! * Adjacent character parts are joined again once markers are in place.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::TextPart;

use crate::draw::Problem;

/// The highest `IX` of a `Character` or `Paragraph` row a marker names: a higher one is treated as absent by a reader.
pub(crate) const MAX_ROW_IX: u32 = 65_535;

/// Which markers a split is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `cp`, a run: a row of `Character`.
    Run,
    /// `pp`, a paragraph: a row of `Paragraph`.
    Paragraph,
}

impl Kind {
    pub(crate) fn section(self) -> &'static str {
        match self {
            Kind::Run => "Character",
            Kind::Paragraph => "Paragraph",
        }
    }

    pub(crate) fn marker(self, ix: u32) -> TextPart {
        match self {
            Kind::Run => TextPart::Cp(ix),
            Kind::Paragraph => TextPart::Pp(ix),
        }
    }

    /// The row a part names, if it is a marker of this kind.
    pub(crate) fn ix(self, part: &TextPart) -> Option<u32> {
        match (self, part) {
            (Kind::Run, TextPart::Cp(ix)) | (Kind::Paragraph, TextPart::Pp(ix)) => Some(*ix),
            _ => None,
        }
    }
}

/// The rows a text names with markers of a kind, in order, once each. A set beside the list: a text of many runs is not walked once for each.
pub(crate) fn rows_named(parts: &[TextPart], kind: Kind) -> Vec<u32> {
    let mut seen: HashSet<u32> = HashSet::new();
    parts.iter().filter_map(|p| kind.ix(p)).filter(|ix| seen.insert(*ix)).collect()
}

/// Whether a run (or a paragraph) that holds the characters `run_from..run_to` is in the range `from..to`: it begins before the range ends and ends
/// after the range begins. A run with no characters (a field's, say) is in the range when it stands strictly inside it; at the range's edge it is not,
/// since a field at the edge of a range is in the run it is in, whatever the range says. A split and the reading of a range both use this.
pub(crate) fn run_in_range(run_from: usize, run_to: usize, from: usize, to: usize) -> bool {
    run_from < to && run_to > from
}

/// The rows of the runs (or paragraphs) of a kind that hold the characters `from..to`, each once, in the order of the text (
/// for `shape.formatting` over a range). **One pass over the parts**, stopping at the first marker at or after `to`; the characters of a part are
/// counted and not looked at. A row that many markers name is listed once (a set beside the list), so a text of a million markers that name two rows
/// gives two. A run with no characters counts only when it holds a field (the field shows text in that run's formatting) and stands strictly inside
/// the range ([`run_in_range`]): an empty run with nothing in it shows nothing, so it does not make the range mixed.
pub(crate) fn rows_in_range(parts: &[TextPart], kind: Kind, from: usize, to: usize) -> Vec<u32> {
    struct Found {
        from: usize,
        to: usize,
        rows: Vec<u32>,
        seen: HashSet<u32>,
    }
    impl Found {
        /// The run in force ended at `end`.
        fn close(&mut self, row: u32, began: usize, end: usize, field: bool) {
            let shows = began < end || field;
            if shows && run_in_range(began, end, self.from, self.to) && self.seen.insert(row) {
                self.rows.push(row);
            }
        }
    }
    let mut found = Found { from, to, rows: Vec::new(), seen: HashSet::new() };
    // The run in force: its row (0 before the first marker), where it began in characters, and whether it holds a field.
    let (mut row, mut began, mut field, mut pos) = (0_u32, 0_usize, false, 0_usize);
    for part in parts {
        #[cfg(test)]
        super::counts::scanned(kind);
        if let Some(ix) = kind.ix(part) {
            found.close(row, began, pos, field);
            if pos >= to {
                // Every run from here on begins at or after the end of the range.
                return found.rows;
            }
            (row, began, field) = (ix, pos, false);
        } else {
            match part {
                TextPart::Chars(_) => pos = pos.saturating_add(chars_in(part)),
                TextPart::Fld { .. } => field = true,
                _ => {}
            }
        }
    }
    found.close(row, began, pos, field);
    found.rows
}

/// The row of the last marker of a kind in the text, row 0 when there is none: the run or paragraph in force at the end of the text.
pub(crate) fn last_row(parts: &[TextPart], kind: Kind) -> u32 {
    parts.iter().rev().find_map(|p| kind.ix(p)).unwrap_or(0)
}

/// The `Character` and `Paragraph` rows that characters typed into a text with none would join: **the last marker of each kind among the markers that
/// open the text** (the characters go after them and ahead of any field), row 0 for a kind with no marker there.
pub(crate) fn rows_when_empty(parts: &[TextPart]) -> (u32, u32) {
    let mut rows = (0, 0);
    for part in parts.iter().take_while(|p| matches!(p, TextPart::Cp(_) | TextPart::Pp(_) | TextPart::Tp(_))) {
        match part {
            TextPart::Cp(ix) => rows.0 = *ix,
            TextPart::Pp(ix) => rows.1 = *ix,
            _ => {}
        }
    }
    rows
}

/// Most rows a text may name, and most a command writes for one kind of marker in one shape: the numbers from 0 to [`MAX_ROW_IX`], which is all a
/// reader takes and so all a section of a text can hold. A text that names more is refused where it would have to be written.
pub(crate) const MAX_NAMED_ROWS: usize = MAX_ROW_IX as usize + 1;

/// The refusal for a text that needs more rows than a section numbers.
pub(crate) fn too_many_rows() -> Problem {
    Problem::failed(format!("the text has too many runs or paragraphs (rows are numbered up to {MAX_ROW_IX})"))
}

/// Gives the rows the markers of a kind name the numbers 0, 1, 2 and so on **in the order they first appear in the text**, which is how a file
/// numbers them, and gives each marker the number of its row: *rows* are renumbered, not markers, so markers that name one row
/// go on naming one row, and the numbers stay within the rows there are, however many markers a text holds. `named` is `rows_named(parts, kind)`.
pub(crate) fn number_rows_in_text_order(parts: &mut [TextPart], kind: Kind, named: &[u32]) {
    let new_of: HashMap<u32, u32> = named.iter().zip(0_u32..).map(|(row, new)| (*row, new)).collect();
    for part in parts.iter_mut() {
        if let Some(new) = kind.ix(part).and_then(|row| new_of.get(&row).copied()) {
            *part = kind.marker(new);
        }
    }
}

/// Gives out new `IX`es in order.
pub(crate) struct Alloc {
    next: u32,
}

impl Alloc {
    /// An allocator whose first `IX` is `next`.
    pub(crate) fn starting_at(next: u32) -> Alloc {
        Alloc { next }
    }

    fn take(&mut self) -> Result<u32, Problem> {
        if self.next > MAX_ROW_IX {
            return Err(too_many_rows());
        }
        let ix = self.next;
        self.next = self.next.saturating_add(1);
        Ok(ix)
    }
}

/// What a split makes of a text.
#[derive(Debug)]
pub(crate) struct Plan {
    /// The parts after the split; `None` when the text is as it was.
    pub parts: Option<Vec<TextPart>>,
    /// Rows to make as copies of rows that resolve already: (the new `IX`, the `IX` it copies).
    pub copies: Vec<(u32, u32)>,
    /// The rows the range's runs use now: the ones to change, in text order, once each.
    pub modify: Vec<u32>,
}

/// How many characters a part holds.
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

/// Makes a boundary between parts at `offset` characters (cutting a character part in two when the offset is inside it) and gives the stretch of
/// part indices that stand at the offset: from just after the character part that ends there to the character part that starts there. Characters
/// parts with no characters count as nothing.
fn zone(w: &mut Vec<TextPart>, offset: usize) -> (usize, usize) {
    let (mut pos, mut lo, mut idx) = (0_usize, 0_usize, 0_usize);
    while let Some(part) = w.get(idx) {
        let n = chars_in(part);
        if n == 0 {
            idx += 1;
            continue;
        }
        if pos >= offset {
            return (lo, idx);
        }
        if pos.saturating_add(n) > offset {
            let TextPart::Chars(s) = part else { return (lo, idx) };
            let (head, tail) = cut(s, offset - pos);
            let (head, tail): (Arc<str>, Arc<str>) = (Arc::from(head), Arc::from(tail));
            w.splice(idx..=idx, [TextPart::Chars(head), TextPart::Chars(tail)]);
            return (idx + 1, idx + 1);
        }
        pos = pos.saturating_add(n);
        lo = idx + 1;
        idx += 1;
    }
    (lo, w.len())
}

/// The row named by the last marker of a kind before index `before`.
fn row_in_force(w: &[TextPart], before: usize, kind: Kind) -> u32 {
    w.get(..before).and_then(|head| head.iter().rev().find_map(|p| kind.ix(p))).unwrap_or(0)
}

/// Whether a marker of the kind stands before the first character.
fn starts_with_marker(w: &[TextPart], kind: Kind) -> bool {
    let first = w.iter().position(|p| chars_in(p) > 0);
    match first {
        None => true,
        Some(first) => w.get(..first).is_some_and(|head| head.iter().any(|p| kind.ix(p).is_some())),
    }
}

/// Splits the text so that the characters `from..to` (`from` less than `to`, both within the text) are in runs or paragraphs of their own, and
/// says which rows to make and which to change (the module says how).
pub(crate) fn plan(parts: &[TextPart], kind: Kind, from: usize, to: usize, alloc: &mut Alloc) -> Result<Plan, Problem> {
    let mut w: Vec<TextPart> = parts.to_vec();
    let mut changed = false;
    // The text starts in row 0 whether or not a marker says so; a marker is written out in the copy so that every character has one before it.
    if !starts_with_marker(&w, kind) {
        // `pp` goes after the `cp` that opens the text, the order a file writes; `cp` goes first.
        let at = match kind {
            Kind::Run => 0,
            Kind::Paragraph => w.iter().take_while(|p| matches!(p, TextPart::Cp(_))).count(),
        };
        w.insert(at, kind.marker(0));
    }

    // The end first: a split there leaves the indices before it where they are. The start's cut may add a part before the end's zone.
    let (lo_e, hi_e) = zone(&mut w, to);
    let parts_before_start = w.len();
    let (lo_s, hi_s) = zone(&mut w, from);
    let shift = w.len().saturating_sub(parts_before_start);
    let (lo_e, hi_e) = (lo_e + shift, hi_e + shift);

    let has_marker = |w: &[TextPart], lo: usize, hi: usize| w.get(lo..hi).is_some_and(|zone| zone.iter().any(|p| kind.ix(p).is_some()));
    let need_end = hi_e < w.len() && !has_marker(&w, lo_e, hi_e);
    let need_start = !has_marker(&w, lo_s, hi_s);
    let end_source = row_in_force(&w, lo_e, kind);
    let start_source = row_in_force(&w, lo_s, kind);
    let at_start = match kind {
        Kind::Run => hi_s,
        Kind::Paragraph => lo_s,
    };

    let mut copies: Vec<(u32, u32)> = Vec::new();
    // New rows are numbered in the order of the text, though the later marker goes in first.
    let start_row = if need_start { Some(alloc.take()?) } else { None };
    let end_row = if need_end { Some(alloc.take()?) } else { None };
    if let Some(row) = end_row {
        w.insert(lo_e, kind.marker(row));
        copies.push((row, end_source));
        changed = true;
    }
    if let Some(row) = start_row {
        w.insert(at_start, kind.marker(row));
        copies.push((row, start_source));
        changed = true;
    }
    // Copies are listed in the order of the text.
    copies.sort_by_key(|(new, _)| *new);

    // The runs now, with where they begin and end in characters.
    struct Run {
        at: usize,
        ix: u32,
        from: usize,
        to: usize,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut pos = 0_usize;
    for (at, part) in w.iter().enumerate() {
        if let Some(ix) = kind.ix(part) {
            if let Some(last) = runs.last_mut() {
                last.to = pos;
            }
            runs.push(Run { at, ix, from: pos, to: usize::MAX });
        } else {
            pos = pos.saturating_add(chars_in(part));
        }
    }
    if let Some(last) = runs.last_mut() {
        last.to = pos;
    }
    let inside = |r: &Run| run_in_range(r.from, r.to, from, to);
    let outside_rows: HashSet<u32> = runs.iter().filter(|r| !inside(r)).map(|r| r.ix).collect();

    // A row that a run in the range shares with one outside it is not changed for both. Sets and a map stand beside the lists, which keep the
    // order: a range that holds thousands of runs is not walked once for each.
    let mut modify: Vec<u32> = Vec::new();
    let mut modify_seen: HashSet<u32> = HashSet::new();
    let mut repointed: HashMap<u32, u32> = HashMap::new();
    for run in runs.iter().filter(|r| inside(r)) {
        let row = if outside_rows.contains(&run.ix) {
            match repointed.get(&run.ix) {
                Some(new) => *new,
                None => {
                    let new = alloc.take()?;
                    copies.push((new, run.ix));
                    repointed.insert(run.ix, new);
                    new
                }
            }
        } else {
            run.ix
        };
        if row != run.ix
            && let Some(slot) = w.get_mut(run.at)
        {
            *slot = kind.marker(row);
            changed = true;
        }
        if modify_seen.insert(row) {
            modify.push(row);
        }
    }

    if !changed {
        return Ok(Plan { parts: None, copies, modify });
    }
    Ok(Plan { parts: Some(joined(w)), copies, modify })
}

/// The parts with neighbouring character parts made one.
fn joined(w: Vec<TextPart>) -> Vec<TextPart> {
    let mut out: Vec<TextPart> = Vec::with_capacity(w.len());
    for part in w {
        match (out.last_mut(), &part) {
            (Some(TextPart::Chars(before)), TextPart::Chars(after)) => {
                let mut both = String::with_capacity(before.len().saturating_add(after.len()));
                both.push_str(before);
                both.push_str(after);
                *before = Arc::from(both);
            }
            _ => out.push(part),
        }
    }
    out
}

/// The paragraph or paragraphs the characters `from..to` are in, as the characters they run from and to (a paragraph ends with its line break, or
/// with the text); `None` when that holds no characters (a point at the end of a text that ends in a line break).
pub(crate) fn paragraph_bounds(parts: &[TextPart], from: usize, to: usize) -> Option<(usize, usize)> {
    let probe = if to > from { to - 1 } else { from };
    let (mut start, mut end) = (0_usize, None::<usize>);
    let mut pos = 0_usize;
    for part in parts {
        let TextPart::Chars(s) = part else { continue };
        for ch in s.chars() {
            if ch == '\n' {
                if pos < from {
                    start = pos + 1;
                }
                if pos >= probe && end.is_none() {
                    end = Some(pos + 1);
                }
            }
            pos = pos.saturating_add(1);
        }
    }
    let end = end.unwrap_or(pos);
    (start < end).then_some((start, end))
}
