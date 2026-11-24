//! From runs to paragraphs of cells: where the text is cut, what is dropped, and what capitals do.
//!
//! A *cell* is one character as it will be laid out: after `Case` and small capitals, with the run it came from and the place it
//! had in the source. Structural characters become cells of their own kind (a tab, a forced break, a soft hyphen); a line feed
//! ends a paragraph and is kept only as which run held it, since that run sizes an empty paragraph.
//!
//! Rules: LF ends a paragraph and a last LF adds no paragraph; U+2028 breaks a line inside one; a stretch
//! that is only the indentation of formatted XML draws nothing; CR and every other control character draw nothing
//! The paragraph's format is the one of the run holding its last character (with the switch below).

use unicode_segmentation::UnicodeSegmentation;

use crate::limits::{MAX_CHARS, MAX_RUNS};
use crate::model::{CharFormat, Run, style};

/// What a cell is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A character that is shaped and drawn.
    Text,
    /// U+0009.
    Tab,
    /// U+2028: ends a line, not a paragraph.
    SoftBreak,
    /// U+00AD: a place to break that shows a hyphen only when the break is taken.
    SoftHyphen,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Cell {
    pub ch: char,
    /// The index of the source character it came from (every character of every run counts, whether drawn or not).
    pub src: u32,
    pub run: u32,
    pub kind: Kind,
    /// A lower-case letter drawn as a small capital.
    pub small_cap: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Paragraph {
    pub cells: Vec<Cell>,
    /// The run whose paragraph format applies.
    pub format_run: u32,
    /// The run holding the paragraph's line feed, if it ends in one: it sizes the paragraph when it is empty.
    pub lf: Option<(u32, u32)>,
}

#[derive(Debug, Default)]
pub(crate) struct Prepared {
    pub paragraphs: Vec<Paragraph>,
    /// The source characters past the cap, not laid out.
    pub dropped: usize,
    /// The runs past the cap, not read.
    pub runs_dropped: usize,
}

/// Which run's format a paragraph takes when several `pp` markers fall inside it. Character rule says the last
/// one before the paragraph's end, as the run holding its last character gives; block rule says the one in force at its first
/// character. The two specs disagree, and the vector is on the first's side, so that is the default. One line to flip.
const PARAGRAPH_FORMAT_FROM_LAST_RUN: bool = true;

/// A stretch holding only spaces, tabs and line ends, with at least one line feed and one space: the file's own indentation.
fn is_indentation(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) && text.contains('\n') && text.contains(' ')
}

/// Is the language Turkish or Azeri, whose capital of `i` carries a dot?
fn dotted_i(lang: &str) -> bool {
    let primary = lang.split(['-', '_']).next().unwrap_or("");
    primary.eq_ignore_ascii_case("tr") || primary.eq_ignore_ascii_case("az")
}

/// The capital of `c`, as one to three characters.
fn capital(c: char, lang: &str) -> ([char; 3], usize) {
    if dotted_i(lang) {
        // The dotless i has `I` for a capital in every language, so only the dotted one is special.
        if c == 'i' {
            return (['\u{130}', '\0', '\0'], 1);
        }
    }
    let mut out = ['\0'; 3];
    let mut n = 0;
    for u in c.to_uppercase().take(3) {
        if let Some(slot) = out.get_mut(n) {
            *slot = u;
            n += 1;
        }
    }
    (out, n)
}

/// The text of the runs one after the other, as far as [`MAX_CHARS`] allows: the text words are found in, since a word may run on
/// from one run into the next (`he` and `llo` are one word).
fn joined_text(runs: &[Run]) -> String {
    let mut out = String::new();
    let mut room = MAX_CHARS;
    for run in runs.iter().take(MAX_RUNS) {
        if room == 0 {
            break;
        }
        let cut = run.text.char_indices().nth(room).map_or(run.text.len(), |(i, _)| i);
        let part = run.text.get(..cut).unwrap_or("");
        room = room.saturating_sub(part.chars().count());
        out.push_str(part);
    }
    out
}

/// For `Case` 2: which characters of `text` start a word that starts with a letter or digit (Unicode word segmentation).
fn word_starts(text: &str) -> Vec<bool> {
    let mut starts = vec![false; text.chars().count()];
    let mut char_index = 0usize;
    for (_, word) in text.split_word_bound_indices() {
        if word.chars().next().is_some_and(char::is_alphanumeric)
            && let Some(s) = starts.get_mut(char_index)
        {
            *s = true;
        }
        char_index += word.chars().count();
    }
    starts
}

/// The cells one character gives, after `Case` and small capitals. Most give one.
fn push_text_cells(out: &mut Vec<Cell>, base: Cell, fmt: &CharFormat, title_start: bool) {
    let small_caps = fmt.style & style::SMALL_CAPS != 0;
    let upper = fmt.case == 1 || (fmt.case == 2 && title_start);
    let small = small_caps && !upper && base.ch.is_lowercase();
    if upper || small {
        let (chars, n) = capital(base.ch, &fmt.lang);
        for ch in chars.into_iter().take(n) {
            out.push(Cell { ch, small_cap: small, ..base });
        }
    } else {
        out.push(base);
    }
}

/// The run whose paragraph format applies to `p`, which ends in a line feed held by `lf_run` or has none.
fn format_run(p: &Paragraph, lf_run: Option<u32>) -> u32 {
    let last = lf_run.or_else(|| p.cells.last().map(|c| c.run));
    let first = p.cells.first().map(|c| c.run).or(lf_run);
    let chosen = if PARAGRAPH_FORMAT_FROM_LAST_RUN { last } else { first };
    chosen.unwrap_or(0)
}

struct Builder {
    done: Vec<Paragraph>,
    cur: Paragraph,
    src: usize,
    dropped: usize,
}

impl Builder {
    fn end_paragraph(&mut self, lf_run: u32) {
        let mut p = std::mem::take(&mut self.cur);
        p.lf = Some((lf_run, u32::try_from(self.src).unwrap_or(u32::MAX)));
        p.format_run = format_run(&p, Some(lf_run));
        self.done.push(p);
    }

    fn character(&mut self, ch: char, run: u32, fmt: &CharFormat, title_start: bool) {
        let src = u32::try_from(self.src).unwrap_or(u32::MAX);
        let base = Cell { ch, src, run, kind: Kind::Text, small_cap: false };
        match ch {
            '\n' => self.end_paragraph(run),
            '\t' => self.cur.cells.push(Cell { kind: Kind::Tab, ..base }),
            '\u{2028}' => self.cur.cells.push(Cell { kind: Kind::SoftBreak, ..base }),
            '\u{00AD}' => self.cur.cells.push(Cell { kind: Kind::SoftHyphen, ..base }),
            // CR, U+2029 and every other control character draw nothing and break nothing.
            c if c.is_control() || c == '\u{2029}' => {}
            _ => push_text_cells(&mut self.cur.cells, base, fmt, title_start),
        }
        self.src += 1;
    }
}

/// Cut `runs` into paragraphs. The caps in [`limits`](crate::limits) bound the work: characters past the cap are counted in
/// `dropped` and not laid out.
pub(crate) fn prepare(runs: &[Run]) -> Prepared {
    let mut b = Builder { done: Vec::new(), cur: Paragraph::default(), src: 0, dropped: 0 };
    // Words are found once, across the runs: a run that ends in the middle of a word does not start another.
    let starts = if runs.iter().take(MAX_RUNS).any(|r| r.char.case == 2) { word_starts(&joined_text(runs)) } else { Vec::new() };
    for (ri, run) in runs.iter().enumerate().take(MAX_RUNS) {
        let ri32 = u32::try_from(ri).unwrap_or(u32::MAX);
        if is_indentation(&run.text) {
            b.src = b.src.saturating_add(run.text.chars().count());
            continue;
        }
        // Only what the cap leaves room for is read: a text of hundreds of megabytes costs a count of its characters, not a pass of
        // case mapping and word segmentation over all of it.
        let room = MAX_CHARS.saturating_sub(b.src);
        let cut = run.text.char_indices().nth(room).map_or(run.text.len(), |(i, _)| i);
        let (text, rest) = run.text.split_at_checked(cut).unwrap_or((&run.text, ""));
        let first_src = b.src;
        for (i, ch) in text.chars().enumerate() {
            let starts_word = run.char.case == 2 && starts.get(first_src + i).copied().unwrap_or(false);
            b.character(ch, ri32, &run.char, starts_word);
        }
        let left_out = rest.chars().count();
        b.dropped = b.dropped.saturating_add(left_out);
        b.src = b.src.saturating_add(left_out);
    }
    if !b.cur.cells.is_empty() {
        let mut p = std::mem::take(&mut b.cur);
        p.format_run = format_run(&p, None);
        b.done.push(p);
    } else if b.done.is_empty() && !runs.is_empty() {
        // Text with nothing in it still takes one line, sized by the first run.
        b.done.push(Paragraph::default());
    }
    Prepared { paragraphs: b.done, dropped: b.dropped, runs_dropped: runs.len().saturating_sub(MAX_RUNS) }
}
