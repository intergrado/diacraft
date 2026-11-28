//! Every cap of the numbers table, counted, with ignored timed twins for the hostile inputs of design section 13.2.

use std::time::{Duration, Instant};

use diacraft_help::catalog::{Catalog, translate};
use diacraft_help::fold::Casing;
use diacraft_help::index::{IndexTopic, TopicIndex};
use diacraft_help::{
    Block, Code, MAX_BLOCKS, MAX_BUNDLE_BYTES, MAX_LINE_BYTES, MAX_SPANS, MAX_TOPIC_BYTES, MAX_TOPICS, Parts, Span, Text, from_parts, parse,
};

use crate::{HEAD, HEAD_LINES, problems, read};

fn count_spans(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(_) | Span::Code(_) => 1,
            Span::Strong(inner) | Span::Em(inner) => 1 + count_spans(inner),
            Span::Link { text, .. } => 1 + count_spans(text),
        })
        .sum()
}

#[test]
fn more_than_4000_blocks_are_cut_and_the_topic_says_so() {
    let mut body = String::new();
    for n in 0..MAX_BLOCKS + 10 {
        body.push_str(&format!("Paragraph {n}.\n\n"));
    }
    let parsed = read(&body);
    assert_eq!(parsed.topic.doc.blocks.len(), MAX_BLOCKS);
    assert!(parsed.topic.doc.cut_short);
    let found: Vec<_> = parsed.problems.iter().map(|p| (p.line, p.code)).collect();
    assert_eq!(found, [(HEAD_LINES + 2 * MAX_BLOCKS + 1, Code::TooManyBlocks)], "said once, at the first block dropped");
    // exactly 4000 are no problem
    let exact: String = (0..MAX_BLOCKS).map(|n| format!("Paragraph {n}.\n\n")).collect();
    let parsed = read(&exact);
    assert_eq!((parsed.topic.doc.blocks.len(), parsed.topic.doc.cut_short, parsed.problems.len()), (MAX_BLOCKS, false, 0));
}

#[test]
fn a_run_of_text_holds_at_most_512_spans() {
    let many = "*a* ".repeat(600);
    let parsed = read(&format!("{many}\n"));
    let Some(Block::Para(text)) = parsed.topic.doc.blocks.first() else { panic!() };
    assert!(count_spans(&text.spans) <= MAX_SPANS, "{}", count_spans(&text.spans));
    assert!(count_spans(&text.spans) >= MAX_SPANS - 3);
    assert_eq!(parsed.problems.iter().map(|p| p.code).collect::<Vec<_>>(), [Code::TooManySpans]);
    // 127 emphasized words with the spaces between them are 381 spans (an emphasis and its text each count): all kept
    let fine = "*a* ".repeat(127);
    assert!(problems(&format!("{fine}\n")).is_empty());
}

#[test]
fn a_line_of_16_kib_is_the_longest_and_a_topic_of_256_kib_the_largest() {
    let line_at = |bytes: usize| format!("{HEAD}{}\n", "a".repeat(bytes));
    assert!(parse(&line_at(MAX_LINE_BYTES)).is_ok());
    let e = parse(&line_at(MAX_LINE_BYTES + 1)).unwrap_err();
    assert_eq!((e.code, e.line), (Code::LineTooLong, HEAD_LINES + 1), "{e}");
    // the same limit holds inside a code block
    let in_code = format!("{HEAD}```text\n{}\n```\n", "a".repeat(MAX_LINE_BYTES + 1));
    assert_eq!(parse(&in_code).unwrap_err().code, Code::LineTooLong);

    let filler_line = format!("{}\n\n", "word ".repeat(100));
    let mut at = String::from(HEAD);
    while at.len() + filler_line.len() < MAX_TOPIC_BYTES {
        at.push_str(&filler_line);
    }
    let pad = MAX_TOPIC_BYTES - at.len();
    at.push_str(&"a".repeat(pad));
    assert_eq!(at.len(), MAX_TOPIC_BYTES);
    assert!(parse(&at).is_ok());
    at.push('a');
    let e = parse(&at).unwrap_err();
    assert_eq!((e.code, e.line), (Code::TopicTooLarge, 0), "{e}");
    assert!(e.to_string().contains("larger than a topic may be"), "{e}");
}

#[test]
fn sixty_thousand_list_items_are_read_in_order() {
    // "- x" and a newline are four bytes: sixty thousand of them are 240,000, under the cap of a topic.
    let body = "- x\n".repeat(60_000);
    let parsed = read(&body);
    assert!(parsed.problems.is_empty());
    let Some(Block::List(list)) = parsed.topic.doc.blocks.first() else { panic!() };
    assert_eq!(list.items.len(), 60_000);
    assert_eq!(list.items[59_999].text.source, "x");
    assert_eq!(list.items[59_999].text.line, HEAD_LINES + 60_000);
}

#[test]
fn a_bundle_holds_a_thousand_topics_and_skips_the_rest_with_a_problem() {
    let texts: Vec<(String, String)> = (0..MAX_TOPICS + 5)
        .map(|n| (format!("t{n}.md"), format!("---\nid: t{n}\ntitle: T\nsummary: S.\nkeywords: k\nsite: no\n---\nText.\n")))
        .collect();
    let refs: Vec<(&str, &str)> = texts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let bundle = from_parts(&Parts { topics: &refs, ..Parts::default() });
    assert_eq!(bundle.ids().len(), MAX_TOPICS);
    assert!(bundle.topic("t999").is_some() && bundle.topic("t1000").is_none());
    let found: Vec<_> = bundle.problems().iter().map(|(_, p)| p.code).collect();
    assert_eq!(found, [Code::BundleTooLarge]);
}

#[test]
fn a_bundle_holds_four_mib_of_topics() {
    let line = format!("{}\n\n", "word ".repeat(100));
    let make = |n: usize| {
        let mut text = format!("---\nid: big{n}\ntitle: T\nsummary: S.\nkeywords: k\nsite: no\n---\n");
        while text.len() + line.len() < 250_000 {
            text.push_str(&line);
        }
        text
    };
    let texts: Vec<(String, String)> = (0..20).map(|n| (format!("big{n}.md"), make(n))).collect();
    let refs: Vec<(&str, &str)> = texts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let bundle = from_parts(&Parts { topics: &refs, ..Parts::default() });
    let total: usize = texts.iter().map(|(_, t)| t.len()).sum();
    assert!(total > MAX_BUNDLE_BYTES);
    let kept = bundle.ids().len();
    assert!(kept < 20 && kept * 250_000 <= MAX_BUNDLE_BYTES + 250_000, "{kept}");
    assert!(bundle.problems().iter().any(|(_, p)| p.code == Code::BundleTooLarge));
}

#[test]
fn a_topic_over_the_cap_a_topic_with_no_header_and_a_repeated_id_are_skipped_and_logged_and_the_rest_are_kept() {
    let good = "---\nid: good\ntitle: T\nsummary: S.\nkeywords: k\nsite: no\n---\nText.\n".to_owned();
    let huge = format!("{good}{}", "a".repeat(MAX_TOPIC_BYTES));
    let twice = good.clone();
    let parts = Parts { topics: &[("good.md", &good), ("huge.md", &huge), ("none.md", "no header"), ("twice.md", &twice)], ..Parts::default() };
    let bundle = from_parts(&parts);
    assert_eq!(bundle.ids(), ["good"]);
    let found: Vec<(&str, Code)> = bundle.problems().iter().map(|(f, p)| (f.as_str(), p.code)).collect();
    assert_eq!(found, [("huge.md", Code::TopicTooLarge), ("none.md", Code::NoHeader), ("twice.md", Code::TopicDuplicate)]);
}

#[test]
fn a_bundle_translates_a_topic_from_its_catalog_and_falls_back_to_english() {
    let src = "---\nid: snap\ntitle: Snap\nsummary: S.\nkeywords: k\nsite: no\n---\nHello.\n";
    let de = "title\tSnap\tEinrasten\npara\tHello.\tHallo.\n";
    let parts = Parts { topics: &[("snap.md", src)], catalogs: &[("de", "snap", de)], ..Parts::default() };
    let bundle = from_parts(&parts);
    let german = bundle.translated("snap", "de").unwrap();
    assert_eq!(german.header.title, "Einrasten");
    let english = bundle.translated("snap", "fr").unwrap();
    assert_eq!(english.header.title, "Snap", "no catalog: English");
    assert!(bundle.translated("nope", "de").is_none());
    assert_eq!(bundle.catalog("de", "snap").map(Catalog::len), Some(2));
}

/// A paragraph of a hundred thousand lines with a fault on each: the faults are placed on their lines in one pass over the lines (a pass for each fault was a hang), and
/// only the first hundred are kept.
#[test]
fn a_paragraph_of_a_hundred_thousand_faulty_lines_places_its_faults_on_their_lines() {
    let body = "]\n".repeat(100_000);
    let parsed = read(&body);
    assert_eq!(parsed.problems.len(), diacraft_help::MAX_PROBLEMS);
    let lines: Vec<usize> = parsed.problems.iter().map(|p| p.line).collect();
    let expected: Vec<usize> = (0..diacraft_help::MAX_PROBLEMS).map(|n| HEAD_LINES + 1 + n).collect();
    assert_eq!(lines, expected);
    assert!(parsed.problems.iter().all(|p| p.code == Code::UnmatchedBracket));
}

/// The numbers of the design's table, pinned: a later task changes one only with a counted test that shows why.
#[test]
fn the_numbers_of_the_table_are_the_designs() {
    use diacraft_help as h;
    assert_eq!((h::MAX_TOPIC_BYTES, h::MAX_TOPICS, h::MAX_BUNDLE_BYTES), (256 * 1024, 1_000, 4 * 1024 * 1024));
    assert_eq!((h::MAX_LINE_BYTES, h::MAX_HEADER_BYTES), (16 * 1024, 8 * 1024));
    assert_eq!((h::MAX_BLOCKS, h::MAX_SPANS), (4_000, 512));
    assert_eq!((h::MAX_LIST_DEPTH, h::MAX_TABLE_COLUMNS, h::MAX_TABLE_ROWS), (2, 6, 60));
    assert_eq!(h::MAX_TARGET_BYTES, 512);
    assert_eq!((h::MAX_QUERY_CHARS, h::MAX_QUERY_WORDS, h::MAX_SCORED), (255, 16, 2_000));
    assert_eq!((h::MAX_COMMAND_ROWS, h::MAX_DRAWING_ROWS, h::HELP_ROWS, h::MAX_TOPIC_RESULTS), (5, 5, 1, 50));
    assert_eq!((h::MAX_INDEX_WORDS, h::MAX_HISTORY), (400_000, 64));
    assert_eq!((h::MAX_TIPS_ROWS, h::MAX_MAP_ROWS, h::MAX_DESCRIPTION_CHARS, h::MAX_KEYWORDS), (5_000, 5_000, 400, 32));
    assert_eq!((h::MAX_CATALOG_BYTES, h::MAX_CATALOG_ROWS, h::MAX_ROW_BYTES), (2 * 1024 * 1024, 20_000, 16 * 1024));
    assert_eq!((h::TIP_REST_MS, h::TIP_SHOWN_MS, h::MAX_FEEDBACK_BODY_BYTES, h::WEB_BUDGET_BYTES), (600, 30_000, 1_500, 256 * 1024));
}

// ---- the timed twins: the inputs of design section 13.2, each of which must finish at once. Run with `-- --ignored`.

fn within(limit: Duration, what: &str, work: impl FnOnce()) {
    let start = Instant::now();
    work();
    let took = start.elapsed();
    assert!(took < limit, "{what} took {took:?}");
}

#[test]
#[ignore = "timed"]
fn timed_a_line_of_stars_brackets_and_backticks() {
    for unit in ["*", "[", "`", "*[`", "[](", "]()", "**_", "\\"] {
        let line = unit.repeat(MAX_LINE_BYTES / unit.len());
        within(Duration::from_secs(2), unit, || {
            let _ = parse(&format!("{HEAD}{line}\n"));
        });
    }
}

#[test]
#[ignore = "timed"]
fn timed_a_paragraph_of_a_hundred_thousand_faulty_lines() {
    let body = format!("{HEAD}{}", "]\n".repeat(100_000));
    within(Duration::from_secs(2), "100,000 faulty lines", || {
        let _ = parse(&body);
    });
    let emphasis = format!("{HEAD}{}", "*a\n".repeat(80_000));
    within(Duration::from_secs(2), "80,000 lines that open emphasis", || {
        let _ = parse(&emphasis);
    });
}

#[test]
#[ignore = "timed"]
fn timed_many_list_markers_and_many_headings() {
    let markers = "- x\n".repeat(MAX_TOPIC_BYTES / 4 - 100);
    within(Duration::from_secs(2), "list markers", || {
        let _ = parse(&format!("{HEAD}{markers}"));
    });
    let nested = "- a\n  - b\n".repeat(MAX_TOPIC_BYTES / 22);
    within(Duration::from_secs(2), "nested markers", || {
        let _ = parse(&format!("{HEAD}{nested}"));
    });
    let headings = "## h\n".repeat(MAX_TOPIC_BYTES / 5 - 100);
    within(Duration::from_secs(2), "headings", || {
        let parsed = parse(&format!("{HEAD}{headings}")).unwrap();
        assert!(parsed.topic.doc.blocks.len() <= MAX_BLOCKS);
    });
}

#[test]
#[ignore = "timed"]
fn timed_a_huge_link_target_and_a_catalog_row_of_many_links() {
    let target = format!("{HEAD}[x](https://diacraft.dev/{})\n", "a".repeat(1 << 20));
    within(Duration::from_secs(2), "1 MiB target", || {
        assert_eq!(parse(&target).unwrap_err().code, Code::TopicTooLarge);
    });
    let long_line = format!("{HEAD}[x](https://diacraft.dev/{})\n", "a".repeat(20_000));
    within(Duration::from_secs(2), "20 KB target", || {
        assert_eq!(parse(&long_line).unwrap_err().code, Code::LineTooLong);
    });
    let links = "[a](topic:b) ".repeat(1_200);
    let src = format!("{HEAD}{links}\n");
    let parsed = parse(&src).unwrap();
    let catalog_text = format!("para\t{}\t{}\n", links.trim_end(), "[a](topic:c) ".repeat(1_200).trim_end());
    let (catalog, _) = Catalog::parse(&catalog_text, false);
    within(Duration::from_secs(2), "a row of links", || {
        let _ = translate(&parsed.topic, &catalog);
    });
}

#[test]
#[ignore = "timed"]
fn timed_a_million_words_for_the_index_and_a_pasted_query() {
    let words = "w1 ".repeat(1_000_000);
    let text = Text { spans: vec![Span::Text(words)], ..Text::default() };
    let topic = diacraft_help::Topic {
        header: diacraft_help::Header { id: "big".to_owned(), title: "Big".to_owned(), ..Default::default() },
        body: String::new(),
        doc: diacraft_help::Doc { blocks: vec![Block::Para(text)], cut_short: false },
    };
    within(Duration::from_secs(3), "indexing a million words", || {
        let index = TopicIndex::build(&[IndexTopic { topic: &topic, english: None, order: 0 }], Casing::Standard);
        let _ = index.search(&"w".repeat(100_000));
    });
}
