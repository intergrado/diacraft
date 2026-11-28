//! The topic index (design section 4.3): the order of what a query matched, its cap and the words it holds.

use diacraft_help::fold::Casing;
use diacraft_help::index::{IndexTopic, MatchField, TopicIndex};
use diacraft_help::{Block, Doc, Header, MAX_INDEX_WORDS, MAX_TOPIC_RESULTS, Span, Text, Topic, parse};

fn topic(id: &str, title: &str, summary: &str, keywords: &str, body: &str) -> Topic {
    let src = format!("---\nid: {id}\ntitle: {title}\nsummary: {summary}\nkeywords: {keywords}\nsite: no\n---\n{body}");
    let parsed = parse(&src).unwrap();
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    parsed.topic
}

fn build(topics: &[Topic]) -> TopicIndex {
    let input: Vec<IndexTopic<'_>> = topics.iter().enumerate().map(|(n, t)| IndexTopic { topic: t, english: None, order: n as u32 }).collect();
    TopicIndex::build(&input, Casing::Standard)
}

fn found(index: &TopicIndex, query: &str) -> Vec<(usize, MatchField)> {
    index.search(query).iter().map(|h| (h.index, h.field)).collect()
}

#[test]
fn the_order_is_title_then_heading_then_keywords_then_summary_then_body() {
    // each topic holds the word only in one place; the input order is the reverse of the ranking
    let topics = [
        topic("body", "Alpha", "Beta.", "gamma", "Text with zebra here.\n"),
        topic("summary", "Alpha", "A zebra.", "gamma", "Text.\n"),
        topic("keywords", "Alpha", "Beta.", "zebra, gamma", "Text.\n"),
        topic("heading", "Alpha", "Beta.", "gamma", "## All about zebra stripes\n\nText.\n"),
        topic("title", "The zebra", "Beta.", "gamma", "Text.\n"),
    ];
    let index = build(&topics);
    assert_eq!(
        found(&index, "zebra"),
        [(4, MatchField::Title), (3, MatchField::Heading), (2, MatchField::Keywords), (1, MatchField::Summary), (0, MatchField::Body)]
    );
}

#[test]
fn a_word_is_the_start_of_a_word_and_case_and_accents_are_ignored() {
    let topics = [topic("a", "Écran partagé", "S.", "k", "Text.\n")];
    let index = build(&topics);
    assert_eq!(found(&index, "ecr").len(), 1);
    assert_eq!(found(&index, "PARTAGE").len(), 1);
    assert!(found(&index, "cran").is_empty());
    assert!(found(&index, "").is_empty());
    assert!(found(&index, "   ").is_empty());
}

#[test]
fn every_word_must_be_found_and_the_weakest_place_decides_the_rank() {
    let topics = [
        topic("one", "Page setup", "S.", "k", "Choose a paper size in the margins list.\n"),
        topic("two", "Page setup", "S.", "k", "Nothing here.\n"),
    ];
    let index = build(&topics);
    // `page` is in the title of both, `margins` only in the body of the first
    assert_eq!(found(&index, "page margins"), [(0, MatchField::Body)]);
    assert_eq!(found(&index, "page setup"), [(0, MatchField::Title), (1, MatchField::Title)]);
    assert!(found(&index, "page zzz").is_empty());
}

#[test]
fn in_a_tie_more_body_words_come_first_and_then_the_order_of_the_home() {
    let topics = [
        topic("few", "T", "S.", "k", "A zebra.\n"),
        topic("many", "T", "S.", "k", "A zebra, another zebra and a third zebra.\n"),
        topic("same", "T", "S.", "k", "A zebra.\n"),
    ];
    let index = build(&topics);
    let order: Vec<usize> = index.search("zebra").iter().map(|h| h.index).collect();
    assert_eq!(order, [1, 0, 2]);
    assert_eq!(index.search("zebra")[0].body_hits, 3);
    // the order given breaks the last tie, not the position in the slice
    let input = [IndexTopic { topic: &topics[0], english: None, order: 9 }, IndexTopic { topic: &topics[2], english: None, order: 2 }];
    let index = TopicIndex::build(&input, Casing::Standard);
    let order: Vec<usize> = index.search("zebra").iter().map(|h| h.index).collect();
    assert_eq!(order, [1, 0]);
}

#[test]
fn the_english_title_and_keywords_are_found_in_a_translated_topic() {
    let german = topic("undo", "Rückgängig", "Nimmt zurück.", "zurück", "Text.\n");
    let english = topic("undo", "Undo", "Takes back.", "revert, step back", "Text.\n");
    let index = TopicIndex::build(&[IndexTopic { topic: &german, english: Some(&english), order: 0 }], Casing::Standard);
    assert_eq!(found(&index, "undo"), [(0, MatchField::Title)]);
    assert_eq!(found(&index, "revert"), [(0, MatchField::Keywords)]);
    assert_eq!(found(&index, "ruckgangig"), [(0, MatchField::Title)]);
    // the English summary and body are not indexed
    assert!(found(&index, "takes").is_empty());
}

#[test]
fn the_text_of_every_kind_of_block_is_indexed() {
    let body = "Intro line.\n\n- first item\n  - nested zeta\n\n> a note about eta\n\n| col | other |\n|---|---|\n| cell theta | x |\n\n```sh\ncommand iota\n```\n\n![alt kappa](site:p)\n";
    let index = build(&[topic("a", "T", "S.", "k", body)]);
    for word in ["intro", "first", "zeta", "eta", "theta", "iota", "kappa"] {
        assert_eq!(found(&index, word), [(0, MatchField::Body)], "{word}");
    }
}

#[test]
fn a_links_target_and_the_markup_are_not_indexed_but_the_text_of_a_link_is() {
    let index = build(&[topic("a", "T", "S.", "k", "See [the glue page](topic:snap-and-glue) and **bold** `code`.\n")]);
    assert_eq!(found(&index, "glue"), [(0, MatchField::Body)]);
    assert_eq!(found(&index, "bold"), [(0, MatchField::Body)]);
    assert_eq!(found(&index, "code"), [(0, MatchField::Body)]);
    assert!(found(&index, "topic").is_empty(), "the word of a target is no word of the text");
    assert!(found(&index, "snap").is_empty());
}

#[test]
fn at_most_fifty_topics_come_back() {
    let topics: Vec<Topic> = (0..80).map(|n| topic(&format!("t{n}"), "Zebra", "S.", "k", "Text.\n")).collect();
    let index = build(&topics);
    let hits = index.search("zebra");
    assert_eq!(hits.len(), MAX_TOPIC_RESULTS);
    assert_eq!(hits[0].index, 0);
    assert_eq!(hits[49].index, 49);
}

#[test]
fn a_query_longer_than_the_cap_is_cut_and_still_searched_once() {
    let index = build(&[topic("a", "Zebra", "S.", "k", "Text.\n")]);
    let huge = "z".repeat(100_000);
    assert!(index.search(&huge).is_empty());
    let long = format!("zebra {}", " ".repeat(100_000));
    assert_eq!(index.search(&long).len(), 1);
}

#[test]
fn the_index_stops_at_its_word_cap_and_says_so() {
    // A topic built by hand: a million short words is more than the parser would read from one file, and more than an index holds.
    let mut words = String::new();
    for n in 0..1_000_000u32 {
        words.push('w');
        words.push_str(&(n % 100).to_string());
        words.push(' ');
    }
    let text = Text { spans: vec![Span::Text(words)], ..Text::default() };
    let big = Topic {
        header: Header { id: "big".to_owned(), title: "Big".to_owned(), ..Header::default() },
        body: String::new(),
        doc: Doc { blocks: vec![Block::Para(text)], cut_short: false },
    };
    let index = TopicIndex::build(&[IndexTopic { topic: &big, english: None, order: 0 }], Casing::Standard);
    assert!(index.truncated, "a million words are more than the cap");
    assert!(index.word_count() <= MAX_INDEX_WORDS, "{}", index.word_count());
    assert!(index.word_count() >= MAX_INDEX_WORDS - 10, "{}", index.word_count());
    // what was indexed is found
    assert_eq!(index.search("w5").len(), 1);
    let small = build(&[topic("a", "Zebra", "S.", "k", "Text.\n")]);
    assert!(!small.truncated);
}

#[test]
fn characters_of_a_language_with_no_spaces_are_found_from_any_of_them() {
    let jp = topic("g", "図形の結合", "要約", "結合", "本文です。\n");
    let index = build(&[jp]);
    assert_eq!(found(&index, "図形"), [(0, MatchField::Title)]);
    assert_eq!(found(&index, "結合"), [(0, MatchField::Title)]);
}
