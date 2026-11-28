//! Properties over arbitrary input: nothing panics, every cap holds, and what the parser calls a segment reads back as itself.

use diacraft_help::catalog::{Catalog, pack, translate, unpack};
use diacraft_help::chord::Chord;
use diacraft_help::fold::{Casing, cut_query, fold, query_words};
use diacraft_help::index::{IndexTopic, TopicIndex};
use diacraft_help::md::{parse_inlines, slug};
use diacraft_help::nav::{History, next_row};
use diacraft_help::rank::{Candidate, rank};
use diacraft_help::tables::{Map, Place, Sections, Tips};
use diacraft_help::url::{allowed, encode_body};
use diacraft_help::{
    Block, MAX_BLOCKS, MAX_HISTORY, MAX_PROBLEMS, MAX_QUERY_CHARS, MAX_SPANS, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, Span, Topic, host, parse,
};
use proptest::prelude::*;

use crate::HEAD;

/// Text made of the characters Help Markdown cares about, so that the parser's rules are reached.
fn markup() -> impl Strategy<Value = String> {
    let alphabet: Vec<&'static str> = vec![
        "*",
        "**",
        "_",
        "[",
        "]",
        "(",
        ")",
        "`",
        "```",
        "\\",
        "#",
        "## ",
        "### ",
        "- ",
        "  ",
        "    ",
        "1. ",
        "> ",
        "|",
        "|---|",
        "!",
        "![",
        "](",
        "<",
        "</",
        "&",
        "\n",
        "\n\n",
        " ",
        "a",
        "b",
        "Z",
        "é",
        "日本",
        "topic:",
        "ctl:",
        "cmd:",
        "key:",
        "https://diacraft.dev/",
        "x.y",
        "{#",
        "}",
        ":",
        "=",
        "---",
        "\t",
        "\u{1}",
    ];
    prop::collection::vec(prop::sample::select(alphabet), 0..120).prop_map(|parts| parts.concat())
}

fn count_spans(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(_) | Span::Code(_) => 1,
            Span::Strong(i) | Span::Em(i) => 1 + count_spans(i),
            Span::Link { text, .. } => 1 + count_spans(text),
        })
        .sum()
}

fn depth(list: &diacraft_help::List) -> usize {
    1 + list.items.iter().filter_map(|i| i.sub.as_ref()).map(depth).max().unwrap_or(0)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn a_topic_of_any_text_reads_within_every_cap(body in markup()) {
        let Ok(parsed) = parse(&format!("{HEAD}{body}")) else { return Ok(()) };
        prop_assert!(parsed.problems.len() <= MAX_PROBLEMS);
        prop_assert!(parsed.topic.doc.blocks.len() <= MAX_BLOCKS);
        for block in &parsed.topic.doc.blocks {
            if let Block::List(list) = block {
                prop_assert!(depth(list) <= 2);
            }
            if let Block::Table { header, rows } = block {
                prop_assert!(header.len() <= MAX_TABLE_COLUMNS && rows.len() < MAX_TABLE_ROWS);
                prop_assert!(rows.iter().all(|r| r.len() <= MAX_TABLE_COLUMNS));
            }
            for text in block.texts() {
                prop_assert!(count_spans(&text.spans) <= MAX_SPANS);
            }
        }
    }

    #[test]
    fn a_topic_that_reads_clean_has_segments_that_read_back_as_themselves(body in markup()) {
        let Ok(parsed) = parse(&format!("{HEAD}{body}")) else { return Ok(()) };
        if !parsed.problems.is_empty() {
            return Ok(());
        }
        for block in &parsed.topic.doc.blocks {
            for text in block.texts() {
                let mut again = Vec::new();
                let spans = parse_inlines(&text.source, 1, &mut again);
                prop_assert!(again.is_empty(), "{:?}: {again:?}", text.source);
                prop_assert_eq!(&spans, &text.spans);
            }
        }
        // and translating with a catalog that gives each segment itself changes nothing
        let mut rows = String::new();
        for segment in diacraft_help::catalog::segments(&parsed.topic) {
            if !segment.source.contains(['\n', '\t', '\\']) {
                rows.push_str(&format!("{}\t{}\t{}\n", segment.context, segment.source, segment.source));
            }
        }
        let (catalog, _) = Catalog::parse(&rows, false);
        let done = translate(&parsed.topic, &catalog);
        prop_assert_eq!(done.topic.doc, parsed.topic.doc);
        prop_assert!(done.skipped.is_empty(), "{:?}", done.skipped);
    }

    #[test]
    fn any_bytes_as_a_topic_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..600)) {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let _ = parse(&text);
        let _ = parse(&format!("{HEAD}{text}"));
        let mut problems = Vec::new();
        let _ = parse_inlines(&text, 1, &mut problems);
    }

    #[test]
    fn a_slug_is_an_ascii_word_list_of_at_most_64_bytes_and_its_own_slug(text in any::<String>()) {
        let s = slug(&text);
        prop_assert!(s.len() <= 64);
        prop_assert!(s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'));
        prop_assert!(!s.starts_with('-') && !s.ends_with('-') && !s.contains("--"));
        prop_assert_eq!(slug(&s), s);
    }

    #[test]
    fn cut_query_gives_a_prefix_on_a_character_boundary(text in any::<String>(), max in 0usize..300) {
        let cut = cut_query(&text, max);
        prop_assert!(text.starts_with(cut));
        prop_assert!(cut.chars().count() <= max);
        prop_assert!(cut.chars().count() == max.min(text.chars().count()));
    }

    #[test]
    fn folding_and_the_words_of_a_query_stay_within_their_caps(text in any::<String>()) {
        for casing in [Casing::Standard, Casing::Turkic] {
            let folded = fold(&text, casing);
            prop_assert!(!folded.starts_with(' ') && !folded.ends_with(' ') && !folded.contains("  "));
            let words = query_words(&text, casing);
            prop_assert!(words.len() <= diacraft_help::MAX_QUERY_WORDS);
            prop_assert!(words.iter().all(|w| !w.is_empty()));
        }
    }

    #[test]
    fn a_ranking_gives_valid_distinct_indexes_within_the_limit(query in any::<String>(), limit in 0usize..8) {
        let place: &[&str] = &["Home", "Arrange"];
        let keywords: &[&str] = &["group", "bundle"];
        let candidates: Vec<Candidate<'_>> = ["Group", "Ungroup", "Align", "Print Preview", "Snap & Glue", "図形を結合"]
            .iter()
            .enumerate()
            .map(|(n, label)| Candidate { label, place, keywords, english: None, order: n as u32, recent: if n % 2 == 0 { Some(n as u32) } else { None } })
            .collect();
        let hits = rank(&query, &candidates, Casing::Standard, limit);
        prop_assert!(hits.len() <= limit);
        let mut seen = std::collections::BTreeSet::new();
        for hit in &hits {
            prop_assert!(hit.index < candidates.len() && seen.insert(hit.index));
        }
    }

    #[test]
    fn a_topic_index_answers_any_query_within_the_cap(query in any::<String>()) {
        let parsed = parse(&format!("{HEAD}Some text about groups and glue.\n")).unwrap();
        let index = TopicIndex::build(&[IndexTopic { topic: &parsed.topic, english: None, order: 0 }], Casing::Standard);
        prop_assert!(index.search(&query).len() <= diacraft_help::MAX_TOPIC_RESULTS);
    }

    #[test]
    fn tables_and_catalogs_of_any_text_never_panic_and_keep_their_caps(text in markup()) {
        let (tips, p1) = Tips::parse(&text);
        let (map, p2) = Map::parse(&text);
        let (sections, p3) = Sections::parse(&text);
        let (catalog, p4) = Catalog::parse(&text, false);
        prop_assert!(tips.rows().len() <= diacraft_help::MAX_TIPS_ROWS && map.rows().len() <= diacraft_help::MAX_MAP_ROWS && catalog.len() <= diacraft_help::MAX_CATALOG_ROWS);
        let _ = sections.sections();
        for p in [&p1, &p2, &p3, &p4] {
            prop_assert!(p.len() <= MAX_PROBLEMS);
        }
        let _ = Place::parse(&text);
    }

    #[test]
    fn a_history_behaves_as_its_model_and_keeps_its_cursor_in_range(ops in prop::collection::vec((0u8..3, 0u8..6), 0..300)) {
        let mut h: History<u8> = History::new();
        let (mut model, mut cursor): (Vec<u8>, usize) = (Vec::new(), 0);
        for (op, place) in ops {
            match op {
                0 => {
                    let changed = h.push(place);
                    if model.get(cursor) == Some(&place) {
                        prop_assert!(!changed);
                    } else {
                        model.truncate(if model.is_empty() { 0 } else { cursor + 1 });
                        model.push(place);
                        if model.len() > MAX_HISTORY {
                            model.remove(0);
                        }
                        cursor = model.len() - 1;
                        prop_assert!(changed);
                    }
                }
                1 => {
                    let got = h.back().copied();
                    if cursor > 0 {
                        cursor -= 1;
                        prop_assert_eq!(got, model.get(cursor).copied());
                    } else {
                        prop_assert_eq!(got, None);
                    }
                }
                _ => {
                    let got = h.forward().copied();
                    if cursor + 1 < model.len() {
                        cursor += 1;
                        prop_assert_eq!(got, model.get(cursor).copied());
                    } else {
                        prop_assert_eq!(got, None);
                    }
                }
            }
            prop_assert!(h.len() <= MAX_HISTORY);
            prop_assert_eq!(h.len(), model.len());
            prop_assert_eq!(h.current().copied(), model.get(cursor).copied());
            prop_assert_eq!(h.can_back(), cursor > 0);
            prop_assert_eq!(h.can_forward(), cursor + 1 < model.len());
        }
    }

    #[test]
    fn back_then_forward_comes_back_to_the_same_place(places in prop::collection::vec(0u8..200, 1..100)) {
        let mut h: History<u8> = History::new();
        for p in places { h.push(p); }
        let here = h.current().copied();
        if h.can_back() {
            h.back();
            h.forward();
            prop_assert_eq!(h.current().copied(), here);
        }
    }

    #[test]
    fn the_row_reached_is_none_or_inside_the_list(current in prop::option::of(any::<usize>()), rows in any::<usize>(), down in any::<bool>()) {
        if let Some(row) = next_row(current, rows, down) {
            prop_assert!(row < rows);
        }
    }

    #[test]
    fn an_encoded_body_is_unreserved_characters_and_escapes_within_the_most(text in any::<String>(), max in 0usize..200) {
        let out = encode_body(&text, max);
        prop_assert!(out.len() <= max);
        prop_assert!(out.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)));
        let mut bytes = out.bytes();
        while let Some(b) = bytes.next() {
            if b == b'%' {
                let (a, c) = (bytes.next(), bytes.next());
                prop_assert!(a.is_some_and(|x| x.is_ascii_hexdigit() && !x.is_ascii_lowercase()) && c.is_some_and(|x| x.is_ascii_hexdigit() && !x.is_ascii_lowercase()));
            }
        }
    }

    #[test]
    fn an_allowed_address_is_plain_ascii_on_one_of_the_sites(url in any::<String>()) {
        if allowed(&url) {
            prop_assert!(url.bytes().all(|b| b.is_ascii_graphic()) && !url.contains('@') && !url.contains('\\'));
            prop_assert!(host::WEB_PREFIXES.iter().any(|p| url.starts_with(p)));
        }
    }

    #[test]
    fn an_allowed_address_has_no_dot_segment_in_its_path_whatever_the_spelling(
        parts in prop::collection::vec(prop::sample::select(vec![".", "..", "/", "%2e", "%2E", "%2f", "%5c", "%25", "%", ";", "a", "x", "?", "#", "%00"]), 0..14),
    ) {
        let url = format!("https://diacraft.dev/{}", parts.concat());
        if allowed(&url) {
            let path = url["https://diacraft.dev".len()..].split(['?', '#']).next().unwrap_or_default().to_owned();
            // An independent reading: take the escapes out up to three times and look at each piece between separators.
            let mut text = path;
            for _ in 0..3 {
                for piece in text.split(['/', '\\']) {
                    let name = piece.split(';').next().unwrap_or_default();
                    prop_assert!(name != "." && name != "..", "{url}");
                }
                text = text.replace("%2e", ".").replace("%2E", ".").replace("%25", "%").replace("%2f", "/").replace("%2F", "/").replace("%5c", "/").replace("%5C", "/");
            }
        }
    }

    #[test]
    fn a_chord_that_reads_writes_text_that_reads_back_the_same(text in any::<String>()) {
        if let Some(c) = Chord::parse(&text) {
            prop_assert_eq!(Chord::parse(&c.canonical()), Some(c));
        }
    }

    #[test]
    fn a_packed_file_is_never_trusted_and_packing_loses_nothing(text in any::<String>(), a in any::<String>(), b in any::<String>()) {
        let _ = unpack(&text);
        let packed = pack(&[("one", a.as_str()), ("two", b.as_str())]);
        let back = unpack(&packed).unwrap();
        prop_assert_eq!(back, vec![("one".to_owned(), a), ("two".to_owned(), b)]);
    }

    #[test]
    fn a_range_is_given_exactly_when_it_fits(start in any::<usize>(), len in any::<usize>(), total in any::<usize>()) {
        let expected = start.checked_add(len).is_some_and(|end| end <= total);
        let got = diacraft_help::catalog::bundle_range(start, len, total);
        prop_assert_eq!(got.is_some(), expected);
        if let Some(r) = got {
            prop_assert!(r.start == start && r.end - r.start == len && r.end <= total);
        }
    }
}

#[test]
fn a_query_is_never_read_past_255_characters() {
    let q = "ab ".repeat(1000);
    let words = query_words(&q, Casing::Standard);
    assert!(words.iter().map(|w| w.len()).sum::<usize>() + words.len() <= MAX_QUERY_CHARS + 1);
}

#[test]
fn a_topic_type_is_cloneable_and_comparable_for_the_tests_above() {
    fn needs<T: Clone + PartialEq + std::fmt::Debug>() {}
    needs::<Topic>();
}
