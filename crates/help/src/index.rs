//! A search index over topics (design: search within help).
//!
//! The index holds, for each topic, the folded words of five places and how often each occurs: the title (and the English title when the topic is translated), the
//! headings, the keywords (and the English keywords), the summary and the body. A query matches a topic when every word of it is the start of a word of the topic in some
//! place; the topic ranks by the weakest place the words needed (every word in the title ahead of a word that needs the body), then by how many words of the body begin a
//! query word, then by its place in the help home. At most [`MAX_TOPIC_RESULTS`] come back. Building is linear in the words, which are capped at [`MAX_INDEX_WORDS`] for
//! the whole index (the rest are left out, and [`TopicIndex::truncated`] says so); a search is linear in the distinct words of the topics times the words of the query.

use std::collections::HashMap;

use crate::block::plain;
use crate::fold::{Casing, fold, query_words, starts_word, words};
use crate::md::{Block, Topic};
use crate::{MAX_INDEX_WORDS, MAX_TOPIC_RESULTS};

/// A topic to index, and where it stands.
#[derive(Clone, Copy, Debug)]
pub struct IndexTopic<'a> {
    /// The topic in the display language.
    pub topic: &'a Topic,
    /// The English topic, when the display language is not English: its title and keywords are indexed too.
    pub english: Option<&'a Topic>,
    /// Where the help home lists it (the order of its section and its place in it).
    pub order: u32,
}

/// Which part of a topic a query matched, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchField {
    /// The title.
    Title,
    /// A heading.
    Heading,
    /// The keywords.
    Keywords,
    /// The summary.
    Summary,
    /// The body.
    Body,
}

/// The number of places a topic is indexed in.
const FIELDS: usize = 5;

/// A topic a query found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopicHit {
    /// The topic's index in what the index was built from.
    pub index: usize,
    /// The weakest part the query needed.
    pub field: MatchField,
    /// How many words of the body begin a query word.
    pub body_hits: u32,
}

#[derive(Debug, Default)]
struct Entry {
    order: u32,
    /// Distinct words and their counts, for each place.
    fields: [Vec<(String, u32)>; FIELDS],
}

/// The index.
#[derive(Debug, Default)]
pub struct TopicIndex {
    entries: Vec<Entry>,
    casing: Option<Casing>,
    words: usize,
    /// Whether words past the cap were left out.
    pub truncated: bool,
}

/// Counts words as they are added, until the budget of the whole index is spent.
struct Counter {
    budget: usize,
    truncated: bool,
}

impl Counter {
    /// Adds the words of `text` into `into`; false once the budget is spent (the rest of `text` is not read).
    fn add(&mut self, into: &mut HashMap<String, u32>, text: &str, casing: Casing) -> bool {
        if self.budget == 0 {
            self.truncated = true;
            return false;
        }
        let folded = fold(text, casing);
        for word in words(&folded) {
            if self.budget == 0 {
                self.truncated = true;
                return false;
            }
            self.budget -= 1;
            *into.entry(word.to_owned()).or_insert(0) += 1;
        }
        true
    }
}

impl TopicIndex {
    /// Builds an index of `topics`, folding by `casing`.
    pub fn build(topics: &[IndexTopic<'_>], casing: Casing) -> TopicIndex {
        let mut counter = Counter { budget: MAX_INDEX_WORDS, truncated: false };
        let mut entries = Vec::with_capacity(topics.len());
        for t in topics {
            let mut maps: [HashMap<String, u32>; FIELDS] = Default::default();
            index_topic(t, casing, &mut counter, &mut maps);
            let fields = maps.map(|m| m.into_iter().collect::<Vec<_>>());
            entries.push(Entry { order: t.order, fields });
        }
        TopicIndex { entries, casing: Some(casing), words: MAX_INDEX_WORDS - counter.budget, truncated: counter.truncated }
    }

    /// How many words are indexed.
    pub fn word_count(&self) -> usize {
        self.words
    }

    /// The topics that match `query`, best first, at most the cap.
    pub fn search(&self, query: &str) -> Vec<TopicHit> {
        let query = query_words(query, self.casing.unwrap_or(Casing::Standard));
        if query.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(TopicHit, u32)> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(hit) = match_entry(index, entry, &query) {
                hits.push((hit, entry.order));
            }
        }
        hits.sort_by_key(|(hit, order)| (hit.field, std::cmp::Reverse(hit.body_hits), *order, hit.index));
        hits.into_iter().take(MAX_TOPIC_RESULTS).map(|(hit, _)| hit).collect()
    }
}

/// Counts the words of one topic into the five places, stopping when the budget is spent.
fn index_topic(t: &IndexTopic<'_>, casing: Casing, counter: &mut Counter, maps: &mut [HashMap<String, u32>; FIELDS]) {
    let [title, heading, keywords, summary, body] = maps;
    let head = &t.topic.header;
    let mut go = counter.add(title, &head.title, casing);
    if let Some(english) = t.english {
        go = go && counter.add(title, &english.header.title, casing) && english.header.keywords.iter().all(|k| counter.add(keywords, k, casing));
    }
    go = go && head.keywords.iter().all(|k| counter.add(keywords, k, casing)) && counter.add(summary, &head.summary, casing);
    for block in &t.topic.doc.blocks {
        if !go {
            return;
        }
        // What a person reads: the text of the spans, never the markup or a link's target.
        go = match block {
            Block::Heading { text, .. } => counter.add(heading, &plain(&text.spans), casing),
            Block::Code { text, .. } => counter.add(body, text, casing),
            Block::Picture { alt, .. } => counter.add(body, alt, casing),
            other => other.texts().iter().all(|text| counter.add(body, &plain(&text.spans), casing)),
        };
    }
}

/// Whether the topic holds every word of the query, and where it needed to look.
fn match_entry(index: usize, entry: &Entry, query: &[String]) -> Option<TopicHit> {
    let mut weakest = MatchField::Title;
    for q in query {
        let best = entry.fields.iter().position(|field| field.iter().any(|(w, _)| starts_word(w, q)))?;
        weakest = weakest.max(field_of(best));
    }
    let body = entry.fields.get(FIELDS - 1).map_or(&[][..], Vec::as_slice);
    let body_hits = body.iter().filter(|(w, _)| query.iter().any(|q| starts_word(w, q))).map(|(_, n)| *n).fold(0u32, u32::saturating_add);
    Some(TopicHit { index, field: weakest, body_hits })
}

fn field_of(position: usize) -> MatchField {
    match position {
        0 => MatchField::Title,
        1 => MatchField::Heading,
        2 => MatchField::Keywords,
        3 => MatchField::Summary,
        _ => MatchField::Body,
    }
}
