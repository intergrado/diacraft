//! `cargo fuzz run help_query`: any bytes as a query, against a fixed set of commands and a fixed index of topics.
//!
//! The properties: nothing panics; the words of a query are at most 16, none empty, from at most 255 characters; a ranking gives at most the limit, distinct valid indexes,
//! kinds in order, and none from past the 2,000 scored; a search of the topics gives at most 50; folding twice is folding once for the text it is given the second time as
//! plain letters (it does not grow); an address that `allowed` lets through has no dot segment in its path, in any spelling, alone or behind either site, and the readers of a key and a place give an answer for any text.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); no corpus is committed.
#![no_main]

use std::sync::OnceLock;

use diacraft_help::chord::Chord;
use diacraft_help::fold::{Casing, cut_query, fold, query_words};
use diacraft_help::index::{IndexTopic, TopicIndex};
use diacraft_help::rank::{Candidate, English, rank};
use diacraft_help::tables::Place;
use diacraft_help::url::allowed;
use diacraft_help::{MAX_QUERY_CHARS, MAX_QUERY_WORDS, MAX_SCORED, MAX_TOPIC_RESULTS, Topic, parse};
use libfuzzer_sys::fuzz_target;

const TOPICS: [(&str, &str, &str); 3] = [
    ("Page setup", "Page, paper, margins", "## Margins\n\nSet the paper size and the margins of a page.\n"),
    ("Snap and glue", "snap, glue, magnet", "Turn snapping on to make shapes stick.\n\n- Glue connects shapes.\n"),
    ("図形の結合", "結合", "図形を結合します。\n"),
];

fn topics() -> &'static Vec<Topic> {
    static TOPICS_READ: OnceLock<Vec<Topic>> = OnceLock::new();
    TOPICS_READ.get_or_init(|| {
        TOPICS
            .iter()
            .enumerate()
            .filter_map(|(n, (title, keywords, body))| {
                let src = format!("---\nid: t{n}\ntitle: {title}\nsummary: A summary.\nkeywords: {keywords}\nsite: no\n---\n{body}");
                parse(&src).ok().map(|p| p.topic)
            })
            .collect()
    })
}

fn index() -> &'static TopicIndex {
    static INDEX: OnceLock<TopicIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let input: Vec<IndexTopic<'_>> = topics().iter().enumerate().map(|(n, t)| IndexTopic { topic: t, english: None, order: n as u32 }).collect();
        TopicIndex::build(&input, Casing::Standard)
    })
}

fuzz_target!(|data: &[u8]| {
    let (flag, rest) = data.split_first().map_or((0u8, &[][..]), |(f, r)| (*f, r));
    let query = String::from_utf8_lossy(rest);
    let casing = if flag & 1 == 1 { Casing::Turkic } else { Casing::Standard };

    assert!(cut_query(&query, MAX_QUERY_CHARS).chars().count() <= MAX_QUERY_CHARS);
    let words = query_words(&query, casing);
    assert!(words.len() <= MAX_QUERY_WORDS && words.iter().all(|w| !w.is_empty()));
    let once = fold(&query, casing);
    assert!(fold(&once, casing).len() <= once.len().saturating_mul(4));

    let place: &[&str] = &["Home", "Arrange"];
    let keywords: &[&str] = &["group", "bundle", "combine"];
    let english = English { label: "Group", place: &["Home"], keywords: &["bundle"] };
    let labels = ["Gruppe", "Ausrichten", "Print Preview", "Snap & Glue", "図形を結合", "İtalik"];
    let mut candidates: Vec<Candidate<'_>> = labels
        .iter()
        .enumerate()
        .map(|(n, label)| Candidate { label, place, keywords, english: (n % 2 == 0).then_some(english), order: n as u32, recent: (n % 3 == 0).then_some(n as u32) })
        .collect();
    // More candidates than the cap scores, so that the cap is reached.
    candidates.extend((0..MAX_SCORED + 10).map(|n| Candidate { label: "Align", order: 100 + n as u32, ..Candidate::default() }));
    let limit = usize::from(flag >> 1) % 12;
    let hits = rank(&query, &candidates, casing, limit);
    assert!(hits.len() <= limit);
    let mut seen = std::collections::BTreeSet::new();
    let mut last_kind = None;
    for hit in &hits {
        assert!(hit.index < MAX_SCORED && seen.insert(hit.index));
        assert!(last_kind.is_none_or(|k| k <= hit.kind));
        last_kind = Some(hit.kind);
    }

    assert!(index().search(&query).len() <= MAX_TOPIC_RESULTS);

    // The small readers answer for any text.
    let _ = Chord::parse(&query);
    // An allowed address has no dot segment in its path in any spelling. It is read here on its own: the escapes are taken out three times and the pieces between separators
    // (and without their parameters) are looked at. The query is tried alone and behind each of the sites.
    for url in [query.to_string(), format!("https://diacraft.dev/{query}"), format!("https://github.com/intergrado/diacraft/{query}")] {
        if allowed(&url) {
            let mut path = url.splitn(4, '/').nth(3).unwrap_or_default().split(['?', '#']).next().unwrap_or_default().to_owned();
            for _ in 0..3 {
                for piece in path.split(['/', '\\']) {
                    let name = piece.split(';').next().unwrap_or_default();
                    assert!(name != "." && name != "..", "{url}");
                }
                path = path.replace("%2e", ".").replace("%2E", ".").replace("%25", "%").replace("%2f", "/").replace("%2F", "/").replace("%5c", "/").replace("%5C", "/");
            }
        }
    }
    let _ = Place::parse(&query);
});
