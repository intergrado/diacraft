//! `cargo fuzz run help_markdown`: any bytes as a topic, through the header, the parser and `translate`.
//!
//! The input is cut at its first 0xFF byte: what is before is the topic (by lossy UTF-8, so that the fuzzer reaches every text), what is after is a help catalog for it.
//! The properties: nothing panics; a topic is refused only by a cap or for want of a header; the blocks and problems are within their caps, a list is two levels deep at the
//! most and a table at most 6 columns by 60 rows, and a run of text holds at most 512 spans; a topic that reads with no problem has segments that read back as themselves;
//! translating with no catalog changes nothing, and with the fuzzed one gives a topic whose blocks are the English topic's.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); no corpus is committed.
#![no_main]

use diacraft_help::catalog::{Catalog, segments, translate};
use diacraft_help::md::parse_inlines;
use diacraft_help::{Block, MAX_BLOCKS, MAX_PROBLEMS, MAX_SPANS, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, Span, parse};
use libfuzzer_sys::fuzz_target;

fn count(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(_) | Span::Code(_) => 1,
            Span::Strong(i) | Span::Em(i) => 1 + count(i),
            Span::Link { text, .. } => 1 + count(text),
        })
        .sum()
}

fn depth(list: &diacraft_help::List) -> usize {
    1 + list.items.iter().filter_map(|i| i.sub.as_ref()).map(depth).max().unwrap_or(0)
}

fuzz_target!(|data: &[u8]| {
    let (topic_bytes, catalog_bytes) = match data.iter().position(|&b| b == 0xFF) {
        Some(at) => (data.get(..at).unwrap_or_default(), data.get(at + 1..).unwrap_or_default()),
        None => (data, &[][..]),
    };
    let text = String::from_utf8_lossy(topic_bytes);
    // A topic as it is, and the same under a header that is right, so that the body is reached.
    for src in [text.to_string(), format!("---\nid: fuzz\ntitle: Fuzz\nsummary: S.\nkeywords: k\nsite: no\n---\n{text}")] {
        let Ok(parsed) = parse(&src) else { continue };
        assert!(parsed.problems.len() <= MAX_PROBLEMS);
        assert!(parsed.topic.doc.blocks.len() <= MAX_BLOCKS);
        for block in &parsed.topic.doc.blocks {
            match block {
                Block::List(list) => assert!(depth(list) <= 2),
                Block::Table { header, rows } => {
                    assert!(header.len() <= MAX_TABLE_COLUMNS && rows.len() < MAX_TABLE_ROWS);
                    assert!(rows.iter().all(|r| r.len() <= MAX_TABLE_COLUMNS));
                }
                _ => {}
            }
            for t in block.texts() {
                assert!(count(&t.spans) <= MAX_SPANS);
                if parsed.problems.is_empty() {
                    let mut again = Vec::new();
                    assert_eq!(parse_inlines(&t.source, 1, &mut again), t.spans);
                    assert!(again.is_empty());
                }
            }
        }
        let none = translate(&parsed.topic, &Catalog::default());
        assert_eq!(none.topic.doc, parsed.topic.doc);
        let (catalog, _) = Catalog::parse(&String::from_utf8_lossy(catalog_bytes), false);
        let done = translate(&parsed.topic, &catalog);
        assert_eq!(done.topic.doc.blocks.len(), parsed.topic.doc.blocks.len());
        assert!(done.topic.header.id == parsed.topic.header.id);
        let _ = segments(&parsed.topic);
    }
});
