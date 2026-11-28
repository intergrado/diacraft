//! `cargo fuzz run help_tables`: any bytes as `tips.tsv`, `map.tsv` and `sections.tsv`.
//!
//! The input, by lossy UTF-8, is read as each of the three tables and as a whole help tree (`from_parts`). The properties: nothing panics; no table has more rows than its
//! cap and no row of one breaks the rule the row would be skipped for (a description of at most 400 characters, at most 32 keywords, a key that is a word, a map row that
//! reads, sections whose titles are not empty); the problems of each are within their cap; and a tree read from the same text twice is the same.
//!
//! The recipe of a run by hand is in `docs/development.md` ("How a target is run by hand"); no corpus is committed.
#![no_main]

use diacraft_help::tables::{Map, Place, Sections, Tips};
use diacraft_help::{MAX_DESCRIPTION_CHARS, MAX_KEYWORDS, MAX_MAP_ROWS, MAX_PROBLEMS, MAX_TIPS_ROWS, Parts, from_parts};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (tips, p1) = Tips::parse(&text);
    assert!(tips.rows().len() <= MAX_TIPS_ROWS && p1.len() <= MAX_PROBLEMS);
    for row in tips.rows() {
        assert!(!row.key.is_empty() && row.description.chars().count() <= MAX_DESCRIPTION_CHARS && row.keywords.len() <= MAX_KEYWORDS);
        assert!(tips.get(&row.key).is_some());
    }
    let (map, p2) = Map::parse(&text);
    assert!(map.rows().len() <= MAX_MAP_ROWS && p2.len() <= MAX_PROBLEMS);
    for row in map.rows() {
        assert_eq!(map.get(&row.place), Some(&row.target));
        assert!(matches!(row.place, Place::Ribbon(_) | Place::Group(_) | Place::Tab(_) | Place::Dialog { .. } | Place::Pane(_) | Place::Backstage(_) | Place::Window(_) | Place::Command(_)));
    }
    let (sections, p3) = Sections::parse(&text);
    assert!(p3.len() <= MAX_PROBLEMS);
    for section in sections.sections() {
        assert!(!section.title.is_empty() && section.topics.len() == section.lines.len() && !section.topics.is_empty());
    }
    let parts = Parts { tips: &text, map: &text, sections: &text, ..Parts::default() };
    let (a, b) = (from_parts(&parts), from_parts(&parts));
    assert_eq!((a.tips().rows().len(), a.map().rows().len(), a.sections().sections().len()), (b.tips().rows().len(), b.map().rows().len(), b.sections().sections().len()));
    assert_eq!(a.problems().len(), b.problems().len());
});
