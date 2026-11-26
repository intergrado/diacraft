//! Search for shapes: the words of a query found in the names and the search words of the masters of every stencil.
//!
//! What matches; the specification's, with the words of a query written out. A *word* is a run of letters and digits, and everything else parts words
//! (`Pump-House` is two). A master matches a query word when the word is **a whole word, in any case,** of the master's name or of its search words: `Air Cond` does not find
//! `Air Conditioner`, and `air` does. With **AND** (`all`, the default: Options, Advanced, Shape search, "All of the words") a master must have every word of the query, with
//! **OR** (`any`) one of them. The same word given twice is one word.
//!
//! **Where the words are read.** A stencil file's master has its **name** (`Name`, else `NameU`) and the text of the **`ShapeKeywords`** cell, read on the master's page sheet
//! (files put it there) and on the first few of its shapes' sheets, as a `V` or as the element's text. A built-in master has its name and the words of
//! [`super::keywords`]. A master that is hidden is not found (the Shapes window leaves it out too).
//!
//! **Which stencils.** The built-in stencils (the ones a drawing has open first, in the window's order, then the others in the catalogue's: a stencil a template left closed
//! is a stencil DiaCraft ships, which a search finds) and then the stencil files that are open, in the order they were opened.
//!
//! **Costs** (a hostile stencil: 100,000 masters, names and search words of megabytes). The query is at most [`MAX_QUERY_CHARS`] characters and [`MAX_QUERY_WORDS`] different
//! words of at most [`MAX_WORD_CHARS`]; a query past them is refused in words (a word dropped from an AND would find more, not fewer). The search looks at **at most
//! [`MAX_SEARCHED_MASTERS`] masters and reads at most [`MAX_SEARCHED_BYTES`] of names and search words**: each master is charged for the text it is read from **before it is read**,
//! a search word text past [`MAX_KEYWORD_BYTES`] is read to there and no further, and a search that meets either limit **stops and says so** (`complete` is false, and the
//! number of masters looked at is given), so a limit that is reached never lets more through. A word of a master is compared with the query's at most [`MAX_QUERY_WORDS`]
//! words, and **a master stops being read as soon as it is decided**, so the work is the bytes charged times 16 at the most, and nothing in it is quadratic in the stencil.
//! At most [`MAX_LISTED`] masters are listed; `matches` counts the rest.

use std::ops::ControlFlow;

use diacraft_doc::Master;
use serde_json::{Value, json};

use super::opened::{OpenedStencil, OpenedStencils};
use super::{StencilSpec, keywords, master_json, stencils_not_in};

/// The longest query, in characters.
pub const MAX_QUERY_CHARS: usize = 256;
/// The most different words a query holds.
pub const MAX_QUERY_WORDS: usize = 16;
/// The longest word of a query, in characters. A longer word of a master is not compared (no query word can equal it).
pub const MAX_WORD_CHARS: usize = 64;
/// The most masters one search looks at, over all the stencils.
pub const MAX_SEARCHED_MASTERS: usize = 100_000;
/// The most bytes of names and search words one search reads, over all the stencils.
pub const MAX_SEARCHED_BYTES: usize = 16 * 1024 * 1024;
/// The most bytes of one search-words cell that are read; the rest of it is not looked at.
pub const MAX_KEYWORD_BYTES: usize = 4_096;
/// The most masters an answer lists.
pub const MAX_LISTED: usize = 500;
/// The shapes of a master whose search words are read (its page sheet is read besides): the ones a listing sizes.
const KEYWORD_SHAPES: usize = 8;
/// The shapes of a master that are looked at to find them: the first of them, deleted or not (a master of thousands of deleted shapes costs this much, and no more).
const KEYWORD_SCAN: usize = 64;

/// Why a query is not one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryProblem {
    /// No word in it: blank, or only punctuation.
    NoWords,
    /// More than [`MAX_QUERY_CHARS`] characters.
    TooLong,
    /// More than [`MAX_QUERY_WORDS`] different words.
    TooManyWords,
    /// A word of more than [`MAX_WORD_CHARS`] characters.
    WordTooLong,
}

impl QueryProblem {
    /// The refusal, in words.
    pub fn words(self) -> String {
        match self {
            QueryProblem::NoWords => "`text` has no words in it: give letters or digits to look for".to_owned(),
            QueryProblem::TooLong => {
                format!("`text` is longer than {MAX_QUERY_CHARS} characters: a search takes at most {MAX_QUERY_CHARS} characters")
            }
            QueryProblem::TooManyWords => {
                format!("`text` has more than {MAX_QUERY_WORDS} different words: a search takes at most {MAX_QUERY_WORDS} different words")
            }
            QueryProblem::WordTooLong => {
                format!("`text` has a word of more than {MAX_WORD_CHARS} characters: a word of a search is at most {MAX_WORD_CHARS} characters")
            }
        }
    }
}

/// A word of a text.
enum Word<'a> {
    /// The word, in lower case.
    Fits(&'a str),
    /// A word of more than [`MAX_WORD_CHARS`] characters, which is not given.
    TooLong,
}

/// Calls `f` with each word of `text`, in order, until it says [`ControlFlow::Break`]. **One pass over the text, and one buffer** that holds at most one word
/// ([`MAX_WORD_CHARS`] characters): no allocation for a word, and a word of a million letters costs a pass over its letters and no more memory.
fn each_word<B>(text: &str, mut f: impl FnMut(Word<'_>) -> ControlFlow<B>) -> ControlFlow<B> {
    let mut buffer = String::with_capacity(MAX_WORD_CHARS * 2);
    let (mut chars, mut over) = (0_usize, false);
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() {
            if chars < MAX_WORD_CHARS {
                buffer.extend(c.to_lowercase());
                chars += 1;
            } else {
                over = true;
            }
            continue;
        }
        if over {
            f(Word::TooLong)?;
        } else if chars > 0 {
            f(Word::Fits(&buffer))?;
        }
        buffer.clear();
        (chars, over) = (0, false);
    }
    ControlFlow::Continue(())
}

/// What is looked for: the different words of a query, and whether all of them or any one decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    words: Vec<String>,
    all: bool,
}

impl Query {
    /// The query `text` makes; `all` is AND, and `false` is OR.
    pub fn parse(text: &str, all: bool) -> Result<Query, QueryProblem> {
        // A text of more than the longest query is refused after a look at its first characters only.
        if text.chars().nth(MAX_QUERY_CHARS).is_some() {
            return Err(QueryProblem::TooLong);
        }
        let mut words: Vec<String> = Vec::new();
        let mut problem = None;
        let _ = each_word(text, |word| match word {
            Word::TooLong => {
                problem = Some(QueryProblem::WordTooLong);
                ControlFlow::Break(())
            }
            Word::Fits(w) => {
                if !words.iter().any(|known| known == w) {
                    if words.len() >= MAX_QUERY_WORDS {
                        problem = Some(QueryProblem::TooManyWords);
                        return ControlFlow::Break(());
                    }
                    words.push(w.to_owned());
                }
                ControlFlow::Continue(())
            }
        });
        if let Some(problem) = problem {
            return Err(problem);
        }
        if words.is_empty() {
            return Err(QueryProblem::NoWords);
        }
        Ok(Query { words, all })
    }

    /// The different words, in lower case, in the order they were given.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Whether every word must be found (AND), and not any one (OR).
    pub fn is_all(&self) -> bool {
        self.all
    }

    /// The bits of the query's words that `text` holds, added to `mask`; stops at once when `mask` decides the master. Returns whether it is decided.
    fn read(&self, text: &str, mask: &mut u32, tokens: &mut u64) -> bool {
        let flow = each_word(text, |word| {
            if let Word::Fits(w) = word {
                *tokens = tokens.saturating_add(1);
                for (i, known) in self.words.iter().enumerate() {
                    if known == w {
                        *mask |= 1_u32 << i;
                    }
                }
                if self.decided(*mask) {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        });
        flow.is_break()
    }

    /// Whether the words found decide that the master matches.
    fn decided(&self, mask: u32) -> bool {
        if self.all {
            // `words.len() <= MAX_QUERY_WORDS`, so the shift cannot overflow.
            mask == (1_u32 << self.words.len()) - 1
        } else {
            mask != 0
        }
    }
}

/// What a search is charged against, and what stopped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// [`MAX_SEARCHED_MASTERS`] masters had been looked at.
    Masters,
    /// [`MAX_SEARCHED_BYTES`] bytes had been read.
    Bytes,
}

/// What is left of the budget of one search.
struct Budget {
    masters: usize,
    bytes: usize,
    stop: Option<Stop>,
}

impl Budget {
    fn new() -> Budget {
        Budget { masters: MAX_SEARCHED_MASTERS, bytes: MAX_SEARCHED_BYTES, stop: None }
    }

    /// Takes the room for one master that is read from `bytes` bytes of text → whether there was room. A refusal is remembered and every later one is the same, so a search
    /// that is out of room reads nothing more.
    fn charge(&mut self, bytes: usize) -> bool {
        if self.stop.is_some() {
            return false;
        }
        if self.masters == 0 {
            self.stop = Some(Stop::Masters);
            return false;
        }
        if bytes > self.bytes {
            self.stop = Some(Stop::Bytes);
            return false;
        }
        self.masters -= 1;
        self.bytes -= bytes;
        true
    }
}

/// The stencil's masters that matched.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    /// Whether the stencil is open in the session (a built-in stencil that a template left closed is searched as well).
    pub open: bool,
    /// The masters as `stencil.list` has them.
    pub masters: Vec<Value>,
}

/// What a search found, and what it cost.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub groups: Vec<Group>,
    /// All the masters that matched, listed or not.
    pub matches: usize,
    /// The masters in `groups`: at most [`MAX_LISTED`].
    pub listed: usize,
    /// The masters looked at.
    pub searched: usize,
    /// The masters of all the stencils: what `searched` falls short of when the search stopped.
    pub total: usize,
    /// Why the search stopped before the last master, if it did.
    pub stopped: Option<Stop>,
    /// The words compared with the query's: the work, counted.
    pub tokens: u64,
    /// The bytes of names and search words charged.
    pub bytes: usize,
    /// The masters whose search words were longer than [`MAX_KEYWORD_BYTES`] and were read to there.
    pub keywords_cut: usize,
}

impl Outcome {
    /// What the search says in words about itself: nothing when it was whole and everything was listed.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(stop) = self.stopped {
            let what = match stop {
                Stop::Masters => format!("{MAX_SEARCHED_MASTERS} masters"),
                Stop::Bytes => format!("{} MiB of names and search words", MAX_SEARCHED_BYTES / (1024 * 1024)),
            };
            notes.push(format!(
                "the search looked at {} of {} masters and stopped: it reads at most {what}, so the masters after them were not searched; open fewer stencils or close the large ones",
                self.searched, self.total
            ));
        }
        if self.matches > self.listed {
            notes.push(format!("{} masters match and the first {} are listed: give more words to narrow the search", self.matches, self.listed));
        }
        if self.keywords_cut > 0 {
            notes.push(format!(
                "{} masters have search words longer than {MAX_KEYWORD_BYTES} bytes, of which the first {MAX_KEYWORD_BYTES} bytes were read",
                self.keywords_cut
            ));
        }
        notes
    }
}

/// The text of a sheet's `ShapeKeywords` cell: its element text, else its `V`; nothing for a blank one.
fn cell_text(sheet: &diacraft_doc::Sheet) -> Option<&str> {
    let cell = sheet.cell("ShapeKeywords")?;
    cell.text().or_else(|| cell.value.as_ref().map(|v| v.text())).filter(|t| !t.trim().is_empty())
}

/// The search words of a master of a stencil file: the texts of its `ShapeKeywords` cells (the page sheet's first, then those of its first shapes), each cut to
/// [`MAX_KEYWORD_BYTES`] when it is read. Nothing is copied: the texts are the model's own.
fn keyword_texts(master: &Master) -> Vec<&str> {
    let mut texts: Vec<&str> = Vec::new();
    texts.extend(cell_text(&master.sheet));
    for shape in master.shapes.iter().take(KEYWORD_SCAN).filter(|s| !s.del).take(KEYWORD_SHAPES) {
        texts.extend(cell_text(&shape.sheet));
    }
    texts
}

/// `text` cut to at most `most` bytes, at a character.
fn cut(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    text.get(..text.floor_char_boundary(most)).unwrap_or("")
}

/// The bytes the search would read of a master with this name and these search words, and whether any search words were cut.
fn bytes_of(name: &str, keywords: &[&str]) -> (usize, bool) {
    let mut bytes = name.len();
    let mut was_cut = false;
    for k in keywords {
        bytes = bytes.saturating_add(k.len().min(MAX_KEYWORD_BYTES));
        was_cut |= k.len() > MAX_KEYWORD_BYTES;
    }
    (bytes, was_cut)
}

/// Where a group's masters are collected as the search goes.
struct Collector {
    groups: Vec<Group>,
    matches: usize,
    listed: usize,
    searched: usize,
    tokens: u64,
    bytes: usize,
    keywords_cut: usize,
    budget: Budget,
}

impl Collector {
    /// Reads one master: `name`, and the search words `keywords` → whether it matches. Charged first; `None` when the budget is out (nothing was read).
    fn look(&mut self, query: &Query, name: &str, keywords: &[&str]) -> Option<bool> {
        let (bytes, was_cut) = bytes_of(name, keywords);
        if !self.budget.charge(bytes) {
            return None;
        }
        self.searched += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        self.keywords_cut += usize::from(was_cut);
        let mut mask = 0_u32;
        let mut decided = query.read(name, &mut mask, &mut self.tokens);
        for k in keywords {
            if decided {
                break;
            }
            decided = query.read(cut(k, MAX_KEYWORD_BYTES), &mut mask, &mut self.tokens);
        }
        Some(query.decided(mask))
    }

    /// Records a match in the group `open` (the last), listing its master `listed` while there is room.
    fn matched(&mut self, group: &mut Group, listed: impl FnOnce() -> Value) {
        self.matches += 1;
        if self.listed < MAX_LISTED {
            group.masters.push(listed());
            self.listed += 1;
        }
    }
}

/// One stencil of the search.
enum Source<'a> {
    Builtin(&'static StencilSpec, bool),
    File(&'a OpenedStencil),
}

/// The stencils a search reads, in order: the built-in ones that are open, the other built-in ones, the stencil files.
fn sources(opened: &OpenedStencils) -> Vec<Source<'_>> {
    let open = opened.open_builtins();
    let mut sources: Vec<Source<'_>> = Vec::with_capacity(open.len() + opened.len() + 8);
    sources.extend(open.iter().filter_map(|id| super::builtin_stencil(id)).map(|s| Source::Builtin(s, true)));
    sources.extend(stencils_not_in(open).map(|s| Source::Builtin(s, false)));
    sources.extend(opened.iter().map(|s| Source::File(s)));
    sources
}

/// The masters of all the stencils a search reads.
fn total_masters(sources: &[Source<'_>]) -> usize {
    sources
        .iter()
        .map(|s| match s {
            Source::Builtin(spec, _) => spec.masters().len(),
            Source::File(stencil) => stencil.master_count(),
        })
        .fold(0_usize, usize::saturating_add)
}

/// Searches every stencil of `opened` (and the built-in ones) for `query`.
pub fn search(opened: &OpenedStencils, query: &Query) -> Outcome {
    let sources = sources(opened);
    let total = total_masters(&sources);
    let mut c = Collector { groups: Vec::new(), matches: 0, listed: 0, searched: 0, tokens: 0, bytes: 0, keywords_cut: 0, budget: Budget::new() };
    for source in &sources {
        let mut group = match source {
            Source::Builtin(spec, open) => {
                Group { id: spec.id.to_owned(), name: spec.name.to_owned(), builtin: true, open: *open, masters: Vec::new() }
            }
            Source::File(stencil) => {
                Group { id: stencil.id().to_owned(), name: stencil.name().to_owned(), builtin: false, open: true, masters: Vec::new() }
            }
        };
        match source {
            Source::Builtin(spec, _) => search_builtin(&mut c, &mut group, spec, query),
            Source::File(stencil) => search_file(&mut c, &mut group, stencil, query),
        }
        if !group.masters.is_empty() {
            c.groups.push(group);
        }
        if c.budget.stop.is_some() {
            break;
        }
    }
    Outcome {
        groups: c.groups,
        matches: c.matches,
        listed: c.listed,
        searched: c.searched,
        total,
        stopped: c.budget.stop,
        tokens: c.tokens,
        bytes: c.bytes,
        keywords_cut: c.keywords_cut,
    }
}

fn search_builtin(c: &mut Collector, group: &mut Group, spec: &StencilSpec, query: &Query) {
    // A hidden master (a custom line end) is not looked at: the window leaves it out, as it leaves out a stencil file's.
    for m in spec.masters().iter().filter(|m| !m.is_hidden()) {
        match c.look(query, m.name, &[keywords::of(m.id)]) {
            None => return,
            Some(true) => c.matched(group, || master_json(m)),
            Some(false) => {}
        }
    }
}

fn search_file(c: &mut Collector, group: &mut Group, stencil: &OpenedStencil, query: &Query) {
    let doc = stencil.document();
    for entry in stencil.entries() {
        // A hidden master is not looked at (the window leaves it out), and costs a master of the budget and no text.
        if entry.hidden {
            if !c.budget.charge(0) {
                return;
            }
            c.searched += 1;
            continue;
        }
        let keywords = doc.masters.get(entry.position).map(|m| keyword_texts(m)).unwrap_or_default();
        match c.look(query, &entry.name, &keywords) {
            None => return,
            Some(true) => c.matched(group, || entry.to_json()),
            Some(false) => {}
        }
    }
}

/// The answer of `stencil.search`.
pub fn answer(text: &str, query: &Query, outcome: &Outcome) -> Value {
    let results: Vec<Value> =
        outcome.groups.iter().map(|g| json!({ "id": g.id, "name": g.name, "builtin": g.builtin, "open": g.open, "masters": g.masters })).collect();
    let mut v = json!({
        "text": crate::text::shown(text, MAX_QUERY_CHARS),
        "mode": if query.is_all() { "all" } else { "any" },
        "words": query.words(),
        "matches": outcome.matches,
        "listed": outcome.listed,
        "truncated": outcome.matches > outcome.listed,
        "searched": outcome.searched,
        "total": outcome.total,
        "complete": outcome.stopped.is_none(),
        "results": results,
    });
    let notes = outcome.notes();
    if let (false, Some(map)) = (notes.is_empty(), v.as_object_mut()) {
        map.insert("notes".to_owned(), json!(notes));
    }
    v
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
