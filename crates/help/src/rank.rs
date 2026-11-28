//! Ranking commands against a query.
//!
//! A candidate matches when every word of the query is the start of a word of its label, of its place (the labels of its tab and group), or of one of its keywords, in the
//! display language or in English (so `bold` finds a command whose label is `Fett`). The order: a label that starts with the query; then a label that holds every word;
//! then the rest. Ties go to the command run from search most recently, then to the host's own order, then to the position given. There is no fuzzy matching of misspellings.
//! At most [`MAX_SCORED`] candidates are looked at, and the work is linear in the candidates times the words of the query.

use crate::MAX_SCORED;
use crate::fold::{Casing, fold, query_words, starts_word, words};

/// What a command is in English, so that an English word finds a command whose label is translated.
#[derive(Clone, Copy, Debug, Default)]
pub struct English<'a> {
    /// The English label.
    pub label: &'a str,
    /// The English place (tab and group).
    pub place: &'a [&'a str],
    /// The English keywords.
    pub keywords: &'a [&'a str],
}

/// A command to rank.
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidate<'a> {
    /// The label in the display language.
    pub label: &'a str,
    /// The place: the labels of its tab and group.
    pub place: &'a [&'a str],
    /// Its keywords.
    pub keywords: &'a [&'a str],
    /// The English label, place and keywords, when the display language is not English.
    pub english: Option<English<'a>>,
    /// Where it is in the host's order (ribbon order, then the registry's).
    pub order: u32,
    /// How recently it was run from search: 0 the last one, 1 the one before, `None` never.
    pub recent: Option<u32>,
}

/// How a hit matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    /// The label starts with the query.
    LabelStart,
    /// Every word was found in the label.
    Label,
    /// Some word was found only through the keywords or the place.
    Other,
}

/// A candidate that matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Its index in the candidates given.
    pub index: usize,
    /// How it matched.
    pub kind: MatchKind,
}

/// The candidates that match `query`, best first, at most `limit`.
pub fn rank(query: &str, candidates: &[Candidate<'_>], casing: Casing, limit: usize) -> Vec<Hit> {
    let query = query_words(query, casing);
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut hits: Vec<(Hit, u32, u32)> = Vec::new();
    for (index, candidate) in candidates.iter().take(MAX_SCORED).enumerate() {
        if let Some(kind) = classify(&query, candidate, casing) {
            hits.push((Hit { index, kind }, candidate.recent.unwrap_or(u32::MAX), candidate.order));
        }
    }
    hits.sort_by_key(|(hit, recent, order)| (hit.kind, *recent, *order, hit.index));
    hits.into_iter().take(limit).map(|(hit, _, _)| hit).collect()
}

/// The words of some texts, folded.
fn folded_words(texts: &[&str], casing: Casing) -> Vec<String> {
    texts.iter().flat_map(|t| words(&fold(t, casing)).map(str::to_owned).collect::<Vec<_>>()).collect()
}

/// How a candidate matches the words of a query, or `None` when it does not.
fn classify(query: &[String], candidate: &Candidate<'_>, casing: Casing) -> Option<MatchKind> {
    let label = fold(candidate.label, casing);
    let english_label = candidate.english.map(|e| fold(e.label, casing)).unwrap_or_default();
    let label_runs: Vec<&str> = words(&label).collect();
    let english_runs: Vec<&str> = words(&english_label).collect();
    let mut others: Vec<String> = folded_words(candidate.place, casing);
    others.extend(folded_words(candidate.keywords, casing));
    if let Some(e) = candidate.english {
        others.extend(folded_words(e.place, casing));
        others.extend(folded_words(e.keywords, casing));
    }
    let in_label = |q: &str| label_runs.iter().chain(&english_runs).any(|w| starts_word(w, q));
    let in_other = |q: &str| others.iter().any(|w| starts_word(w, q));
    if !query.iter().all(|q| in_label(q) || in_other(q)) {
        return None;
    }
    if starts_with_query(&label_runs, query) || starts_with_query(&english_runs, query) {
        return Some(MatchKind::LabelStart);
    }
    Some(if query.iter().all(|q| in_label(q)) { MatchKind::Label } else { MatchKind::Other })
}

/// Whether the words of a label begin with the words of the query, one for one.
fn starts_with_query(label: &[&str], query: &[String]) -> bool {
    query.len() <= label.len() && label.iter().zip(query).all(|(word, q)| word.starts_with(q.as_str()))
}
