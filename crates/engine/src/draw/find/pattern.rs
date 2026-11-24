//! What Find looks for: the typed text read into a pattern, and the search of one string for it.
//!
//! **The typed text.** `Find what` is literal text, with the special characters the dialog's Special list puts into it (a tab, a manual
//! line break, an optional hyphen, and any character as a wildcard). **The spelling of those four is ours** (the specification names the characters and not what is
//! typed for them: question): `^t` is a tab, `^l` a manual line break (U+2028, where a plain line feed ends a paragraph), `^-` an optional hyphen (U+00AD),
//! `^?` any one character, and `^^` is a caret. A caret before any other character is a caret.
//!
//! **What matches.** A match is a run of characters of the text that the pattern's characters equal one for one, so its length in characters is the pattern's.
//! *Match case* compares the characters as stored, whatever caps formatting shows (E3 of the inventory); without it a character equals its lower case when that is a single
//! character. *Whole words only* needs a match not to touch a word character (a letter, a digit or `_`) on either side, so `for` is in `for` and not in `forest`
//! Matches never overlap: the search goes on after the end of one.
//!
//! **What it costs.** A search is charged to a [`Work`] before it is done, a step for each character it passes and each character it compares, and refused in words past
//! the limit. The pattern is at most [`MAX_PATTERN_CHARS`] characters, and the worst text for the plain loop (a long run of the pattern's first character) is
//! charged for what it costs, so a hostile pattern and text cannot take long. A pattern that is plain text and matches case is found by `str::find`, which is linear.

/// The most characters of a pattern.
pub const MAX_PATTERN_CHARS: usize = 1_024;

/// A budget of steps for one search: charged before the work is done.
#[derive(Debug, Clone)]
pub struct Work {
    used: u64,
    limit: u64,
    /// How many sections of rows the search left unread because a shape had more rows than it reads: told in the answer, never dropped silently.
    unread: u64,
}

/// A search that would pass its [`Work`]'s limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused;

impl Work {
    /// A budget of `limit` steps.
    pub fn new(limit: u64) -> Work {
        Work { used: 0, limit, unread: 0 }
    }

    /// Takes `steps` off the budget: `Err` when that passes the limit.
    pub fn charge(&mut self, steps: u64) -> Result<(), Refused> {
        self.used = self.used.saturating_add(steps);
        if self.used > self.limit { Err(Refused) } else { Ok(()) }
    }

    /// The steps charged so far.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Notes a section of rows that was too long to read.
    pub fn note_unread(&mut self) {
        self.unread = self.unread.saturating_add(1);
    }

    /// How many sections of rows were left unread.
    pub fn unread(&self) -> u64 {
        self.unread
    }

    /// The limit.
    pub fn limit(&self) -> u64 {
        self.limit
    }
}

/// A character of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    /// This character (folded to lower case when the pattern does not match case).
    Lit(char),
    /// Any one character.
    Any,
}

/// A pattern, with the options that decide what matches it.
#[derive(Clone, Debug)]
pub struct Pattern {
    toks: Vec<Tok>,
    /// The text of the pattern when it is plain characters and matches case: the search is `str::find`.
    literal: Option<String>,
    match_case: bool,
    whole_words: bool,
}

/// Whether `c` is a word character for Find whole words only: a letter, a digit or `_`.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `c` in lower case when that is one character (so a match is character for character), else `c`.
fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(one), None) => one,
        _ => c,
    }
}

impl Pattern {
    /// The pattern `what` names. Refused in words when it is empty or longer than [`MAX_PATTERN_CHARS`] characters.
    pub fn parse(what: &str, match_case: bool, whole_words: bool) -> Result<Pattern, String> {
        let mut toks: Vec<Tok> = Vec::new();
        let mut chars = what.chars().peekable();
        while let Some(c) = chars.next() {
            if toks.len() >= MAX_PATTERN_CHARS {
                return Err(format!("`what` must be at most {MAX_PATTERN_CHARS} characters"));
            }
            let tok = if c == '^' {
                match chars.peek().copied() {
                    Some('t') => Tok::Lit('\t'),
                    Some('l') => Tok::Lit('\u{2028}'),
                    Some('-') => Tok::Lit('\u{ad}'),
                    Some('?') => Tok::Any,
                    Some('^') => Tok::Lit('^'),
                    _ => Tok::Lit('^'),
                }
            } else {
                Tok::Lit(c)
            };
            // A caret that starts a special takes the character after it.
            if c == '^' && matches!(chars.peek().copied(), Some('t' | 'l' | '-' | '?' | '^')) {
                chars.next();
            }
            toks.push(match tok {
                Tok::Lit(l) if !match_case => Tok::Lit(fold(l)),
                other => other,
            });
        }
        if toks.is_empty() {
            return Err("`what` must hold something to find".to_owned());
        }
        let literal = (match_case && toks.iter().all(|t| matches!(t, Tok::Lit(_)))).then(|| {
            toks.iter()
                .filter_map(|t| match t {
                    Tok::Lit(c) => Some(*c),
                    Tok::Any => None,
                })
                .collect::<String>()
        });
        Ok(Pattern { toks, literal, match_case, whole_words })
    }

    /// How many characters a match has.
    pub fn len(&self) -> usize {
        self.toks.len()
    }

    /// A pattern is never empty (see [`Pattern::parse`]).
    pub fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    fn tok_matches(&self, tok: Tok, c: char) -> bool {
        match tok {
            Tok::Any => true,
            Tok::Lit(l) if self.match_case => l == c,
            Tok::Lit(l) => l == fold(c),
        }
    }

    /// Whether a run of the text that starts after `prev` and ends before `next` is a match as far as whole words go.
    fn bounded(&self, prev: Option<char>, next: Option<char>) -> bool {
        !self.whole_words || (!prev.is_some_and(is_word) && !next.is_some_and(is_word))
    }

    /// Every match in `hay` that starts at the character `from` or after it, in order, as `(start, end)` in characters, `each` being called with it: it answers whether to
    /// go on. The characters before `from` are passed over and counted. A search that would pass the budget is refused, and what `each` was given stands.
    pub fn find_in(&self, hay: &str, from: usize, work: &mut Work, mut each: impl FnMut(usize, usize) -> bool) -> Result<(), Refused> {
        // The byte and the character the search is at, and the character before it.
        let (mut byte, mut index) = (0_usize, 0_usize);
        let mut prev: Option<char> = None;
        if from > 0 {
            match hay.char_indices().nth(from) {
                Some((b, _)) => {
                    work.charge(u64::try_from(from).unwrap_or(u64::MAX))?;
                    byte = b;
                    index = from;
                    prev = hay.get(..b).and_then(|h| h.chars().next_back());
                }
                None => {
                    work.charge(u64::try_from(hay.len()).unwrap_or(u64::MAX))?;
                    return Ok(());
                }
            }
        }
        match &self.literal {
            Some(literal) => self.find_literal(literal, hay, (byte, index, prev), work, &mut each),
            None => self.find_general(hay, (byte, index, prev), work, &mut each),
        }
    }

    /// The plain-text case, by `str::find`.
    fn find_literal(
        &self,
        literal: &str,
        hay: &str,
        (mut byte, mut index, mut prev): (usize, usize, Option<char>),
        work: &mut Work,
        each: &mut dyn FnMut(usize, usize) -> bool,
    ) -> Result<(), Refused> {
        let m = self.toks.len();
        loop {
            let Some(rest) = hay.get(byte..) else { return Ok(()) };
            let Some(found) = rest.find(literal) else {
                work.charge(u64::try_from(rest.len()).unwrap_or(u64::MAX))?;
                return Ok(());
            };
            // `str::find` scans bytes; what is charged is the bytes it passed and the characters counted after.
            work.charge(u64::try_from(found).unwrap_or(u64::MAX).saturating_add(u64::try_from(literal.len()).unwrap_or(u64::MAX)))?;
            let before = rest.get(..found).unwrap_or("");
            let start = index.saturating_add(before.chars().count());
            let prev_here = before.chars().next_back().or(prev);
            let after_at = found.saturating_add(literal.len());
            let next = rest.get(after_at..).and_then(|r| r.chars().next());
            if self.bounded(prev_here, next) {
                let end = start.saturating_add(m);
                if !each(start, end) {
                    return Ok(());
                }
                byte = byte.saturating_add(after_at);
                index = end;
                prev = literal.chars().next_back();
            } else {
                // Not a match here: go on from the second character of the run found.
                let first = rest.get(found..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
                byte = byte.saturating_add(found).saturating_add(first);
                index = start.saturating_add(1);
                prev = rest.get(found..).and_then(|r| r.chars().next());
            }
        }
    }

    /// Every other case: a loop over the characters.
    fn find_general(
        &self,
        hay: &str,
        (mut byte, mut index, mut prev): (usize, usize, Option<char>),
        work: &mut Work,
        each: &mut dyn FnMut(usize, usize) -> bool,
    ) -> Result<(), Refused> {
        let Some(&first) = self.toks.first() else { return Ok(()) };
        let m = self.toks.len();
        loop {
            let Some(rest) = hay.get(byte..) else { return Ok(()) };
            let Some(c0) = rest.chars().next() else { return Ok(()) };
            work.charge(1)?;
            if self.tok_matches(first, c0) {
                let mut it = rest.chars();
                let mut used_bytes = 0_usize;
                let mut last = c0;
                let mut whole_run = true;
                for tok in &self.toks {
                    work.charge(1)?;
                    match it.next() {
                        Some(c) if self.tok_matches(*tok, c) => {
                            used_bytes = used_bytes.saturating_add(c.len_utf8());
                            last = c;
                        }
                        _ => {
                            whole_run = false;
                            break;
                        }
                    }
                }
                if whole_run && self.bounded(prev, it.next()) {
                    let end = index.saturating_add(m);
                    if !each(index, end) {
                        return Ok(());
                    }
                    byte = byte.saturating_add(used_bytes);
                    index = end;
                    prev = Some(last);
                    continue;
                }
            }
            byte = byte.saturating_add(c0.len_utf8());
            index = index.saturating_add(1);
            prev = Some(c0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(what: &str, hay: &str, case: bool, whole: bool) -> Vec<(usize, usize)> {
        let pattern = Pattern::parse(what, case, whole).unwrap();
        let mut out = Vec::new();
        pattern
            .find_in(hay, 0, &mut Work::new(u64::MAX), |s, e| {
                out.push((s, e));
                true
            })
            .unwrap();
        out
    }

    /// The reference: a match at every position that has the characters, then the non-overlap and whole-word rules, on a `Vec<char>`.
    fn reference(what: &str, hay: &str, case: bool, whole: bool) -> Vec<(usize, usize)> {
        let pattern = Pattern::parse(what, case, whole).unwrap();
        let text: Vec<char> = hay.chars().collect();
        let m = pattern.toks.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i + m <= text.len() {
            let here = (0..m).all(|k| pattern.tok_matches(pattern.toks[k], text[i + k]));
            let prev = i.checked_sub(1).map(|j| text[j]);
            let next = text.get(i + m).copied();
            if here && (!whole || (!prev.is_some_and(is_word) && !next.is_some_and(is_word))) {
                out.push((i, i + m));
                i += m;
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn plain_text_is_found_every_time_and_matches_do_not_overlap() {
        assert_eq!(all("ab", "ab ab xab", true, false), vec![(0, 2), (3, 5), (7, 9)]);
        assert_eq!(all("aa", "aaaaa", true, false), vec![(0, 2), (2, 4)], "the search goes on after a match");
    }

    #[test]
    fn match_case_compares_the_characters_as_they_are() {
        assert_eq!(all("cat", "Cat cat CAT", false, false), vec![(0, 3), (4, 7), (8, 11)]);
        assert_eq!(all("cat", "Cat cat CAT", true, false), vec![(4, 7)]);
        assert_eq!(all("É", "é É", false, false), vec![(0, 1), (2, 3)], "a letter with an accent folds too");
    }

    /// V6 of the Home tab specification: with Find whole words only, "for" is found in "for forest" once.
    #[test]
    fn whole_words_only_finds_for_in_for_and_not_in_forest() {
        assert_eq!(all("for", "for forest", false, true), vec![(0, 3)]);
        assert_eq!(all("for", "for forest", false, false), vec![(0, 3), (4, 7)]);
        assert_eq!(all("for", "a_for for_b (for)", false, true), vec![(13, 16)], "an underscore and a digit are word characters, a bracket is not");
        assert_eq!(all("for", "4for for4 for", false, true), vec![(10, 13)]);
    }

    #[test]
    fn the_special_characters_are_the_ones_the_dialog_lists() {
        assert_eq!(all("a^tb", "a\tb ab", true, false), vec![(0, 3)], "a tab");
        assert_eq!(all("a^lb", "a\u{2028}b a\nb", true, false), vec![(0, 3)], "a manual line break is not a paragraph end");
        assert_eq!(all("co^-op", "co\u{ad}op coop", true, false), vec![(0, 5)], "an optional hyphen");
        assert_eq!(all("c^?t", "cat cut ct c\nt", true, false), vec![(0, 3), (4, 7), (11, 14)], "any one character, a line end too");
        assert_eq!(all("2^^3", "2^3 23", true, false), vec![(0, 3)], "a caret");
        assert_eq!(all("a^x", "a^x ax", true, false), vec![(0, 3)], "a caret before another letter is a caret");
        assert_eq!(all("a^", "a^", true, false), vec![(0, 2)], "and a caret at the end");
    }

    #[test]
    fn the_wildcard_and_case_folding_take_the_general_path_and_agree_with_the_reference() {
        let hays = ["", "x", "Hello hello HELLO", "aaaa aab aaab", "ünï çödé ÜNÏ", "a\tb\u{2028}c", "🙂a🙂a🙂", "for forest For_each (for)"];
        let whats = ["a", "aa", "aab", "h^?llo", "^?", "ü", "ï ç", "🙂a", "for", "^t", "b^lc", "^^"];
        for hay in hays {
            for what in whats {
                for case in [true, false] {
                    for whole in [true, false] {
                        assert_eq!(all(what, hay, case, whole), reference(what, hay, case, whole), "{what:?} in {hay:?}, case {case}, whole {whole}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_search_can_start_part_way_and_counts_characters_not_bytes() {
        let pattern = Pattern::parse("é", true, false).unwrap();
        let hay = "éaéaé";
        for (from, want) in
            [(0, vec![(0, 1), (2, 3), (4, 5)]), (1, vec![(2, 3), (4, 5)]), (2, vec![(2, 3), (4, 5)]), (3, vec![(4, 5)]), (5, vec![]), (9, vec![])]
        {
            let mut got = Vec::new();
            pattern
                .find_in(hay, from, &mut Work::new(u64::MAX), |s, e| {
                    got.push((s, e));
                    true
                })
                .unwrap();
            assert_eq!(got, want, "from {from}");
        }
        // From part-way, the character before the start is the one a whole-word check looks at.
        let word = Pattern::parse("for", true, true).unwrap();
        let mut got = Vec::new();
        word.find_in("xfor for", 1, &mut Work::new(u64::MAX), |s, e| {
            got.push((s, e));
            true
        })
        .unwrap();
        assert_eq!(got, vec![(5, 8)], "the f at 1 follows an x, so it is not a whole word");
    }

    #[test]
    fn the_search_stops_when_the_caller_says_so() {
        let pattern = Pattern::parse("a", true, false).unwrap();
        let mut seen = 0;
        pattern
            .find_in("aaaaaaaa", 0, &mut Work::new(u64::MAX), |_, _| {
                seen += 1;
                seen < 3
            })
            .unwrap();
        assert_eq!(seen, 3);
    }

    #[test]
    fn an_empty_or_too_long_pattern_is_refused_in_words() {
        assert_eq!(Pattern::parse("", true, false).unwrap_err(), "`what` must hold something to find");
        let long = "a".repeat(MAX_PATTERN_CHARS + 1);
        assert_eq!(Pattern::parse(&long, true, false).unwrap_err(), format!("`what` must be at most {MAX_PATTERN_CHARS} characters"));
        assert_eq!(Pattern::parse(&"a".repeat(MAX_PATTERN_CHARS), true, false).unwrap().len(), MAX_PATTERN_CHARS);
    }

    /// Counted, not timed: the worst text for the plain loop (the pattern's first character over and over, then a miss) is charged for what it costs, so the budget
    /// stops it, and the cost is the text's length times the pattern's at most.
    #[test]
    fn a_hostile_pattern_and_text_are_charged_for_the_work_and_stopped_by_the_budget() {
        let hay = "a".repeat(100_000);
        let pattern = Pattern::parse(&format!("{}b", "a".repeat(999)), false, false).unwrap();
        let mut work = Work::new(10_000_000);
        let out = pattern.find_in(&hay, 0, &mut work, |_, _| true);
        assert_eq!(out, Err(Refused), "100,000 x 1,000 comparisons pass ten million steps");
        assert!(work.used() > 10_000_000 && work.used() < 10_000_000 + 2_000, "the charge stops where the limit is passed: {}", work.used());
        // The same text with a short pattern costs a few steps for each character.
        let mut work = Work::new(u64::MAX);
        Pattern::parse("ab", false, false).unwrap().find_in(&hay, 0, &mut work, |_, _| true).unwrap();
        assert!(work.used() <= 3 * 100_000, "{}", work.used());
    }

    /// The plain case is linear: `str::find` goes through ten megabytes charging about a step for each byte, and finds every match.
    #[test]
    fn the_plain_case_is_charged_by_bytes_and_finds_every_match_in_ten_megabytes() {
        let hay = format!("{}needle", "x".repeat(10 * 1024 * 1024));
        let pattern = Pattern::parse("needle", true, false).unwrap();
        let mut work = Work::new(u64::MAX);
        let mut got = Vec::new();
        pattern
            .find_in(&hay, 0, &mut work, |s, e| {
                got.push((s, e));
                true
            })
            .unwrap();
        assert_eq!(got, vec![(10 * 1024 * 1024, 10 * 1024 * 1024 + 6)]);
        assert!(work.used() <= 10 * 1024 * 1024 + 64, "about a step for each byte: {}", work.used());
    }
}
