//! The unit tests of the shape search: its words, its limits and what a hostile stencil costs. The files that open and list a stencil are in
//! `crates/engine/tests/it/stencil_search.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, Sheet, Value};

use super::*;
use crate::stencil::opened::ModelCost;

fn query(text: &str) -> Query {
    Query::parse(text, true).unwrap()
}

/// The words of `text` as a plain reference reads them: split at every character that is no letter or digit, and each lower-cased whole.
fn reference_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// A tiny deterministic generator, so the random cases are the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap() % n
    }
}

fn random_text(rng: &mut Lcg, pieces: &[&str], most: usize) -> String {
    let mut out = String::new();
    for _ in 0..rng.pick(most + 1) {
        out.push_str(pieces[rng.pick(pieces.len())]);
    }
    out
}

#[test]
fn a_query_is_its_different_words_in_lower_case() {
    let q = query("Pump  PUMP-house, 42!  Ünï");
    assert_eq!(q.words(), ["pump", "house", "42", "ünï"]);
    assert!(q.is_all());
    assert!(!Query::parse("a", false).unwrap().is_all());
    assert_eq!(Query::parse("", true), Err(QueryProblem::NoWords));
    assert_eq!(Query::parse(" -- ?? ", true), Err(QueryProblem::NoWords));
}

#[test]
fn the_limits_of_a_query_are_exact_in_characters_and_words() {
    // 256 characters is the most, counted in characters and not bytes.
    let spaced = "é ".repeat(MAX_QUERY_CHARS / 2);
    assert_eq!(spaced.chars().count(), MAX_QUERY_CHARS);
    assert!(Query::parse(&spaced, true).is_ok(), "256 characters are accepted");
    assert_eq!(Query::parse(&format!("{spaced}é"), true), Err(QueryProblem::TooLong));
    assert_eq!(Query::parse(&"é".repeat(MAX_QUERY_CHARS + 1), true), Err(QueryProblem::TooLong));
    // 64 characters in a word, 65 is too many.
    assert!(Query::parse(&"a".repeat(MAX_WORD_CHARS), true).is_ok());
    assert_eq!(Query::parse(&"a".repeat(MAX_WORD_CHARS + 1), true), Err(QueryProblem::WordTooLong));
    // 16 different words, and a seventeenth is refused; a repeated word is one.
    let words = |n: usize| (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ");
    assert_eq!(Query::parse(&words(16), true).unwrap().words().len(), 16);
    assert_eq!(Query::parse(&words(17), true), Err(QueryProblem::TooManyWords));
    assert_eq!(Query::parse(&format!("{} w3 W5", words(16)), true).unwrap().words().len(), 16);
}

#[test]
fn a_word_of_a_million_letters_is_one_word_that_is_not_given_and_costs_no_memory() {
    let text = format!("start {} end", "z".repeat(1_000_000));
    let mut seen = Vec::new();
    let _ = each_word::<()>(&text, |w| {
        seen.push(match w {
            Word::Fits(w) => w.to_owned(),
            Word::TooLong => "<too long>".to_owned(),
        });
        ControlFlow::Continue(())
    });
    assert_eq!(seen, ["start", "<too long>", "end"]);
    // A word of exactly the most is given, whole.
    let exact = "q".repeat(MAX_WORD_CHARS);
    let mut got = None;
    let _ = each_word::<()>(&exact, |w| {
        if let Word::Fits(w) = w {
            got = Some(w.len());
        }
        ControlFlow::Continue(())
    });
    assert_eq!(got, Some(MAX_WORD_CHARS));
}

/// What the search decides is what a plain reading of the words decides, for random texts and queries: letters of both cases, digits, accents and separators of several kinds.
#[test]
fn a_master_matches_exactly_when_a_plain_reading_of_its_words_says_so() {
    let pieces = ["a", "B", "pump", "Pump", "É", "é", "7", " ", "-", "_", "; ", "'", "air", "AIR", "cond", "İ", "ß", "x1"];
    let mut rng = Lcg(7);
    let mut found = 0_usize;
    for _ in 0..3_000 {
        let name = random_text(&mut rng, &pieces, 6);
        let keyword = random_text(&mut rng, &pieces, 8);
        let asked = random_text(&mut rng, &pieces, 4);
        for all in [true, false] {
            let Ok(q) = Query::parse(&asked, all) else { continue };
            let have: Vec<String> = reference_words(&name).into_iter().chain(reference_words(&keyword)).collect();
            let has = |w: &String| have.iter().any(|h| h == w);
            let expected = if all { q.words().iter().all(has) } else { q.words().iter().any(has) };
            let mut mask = 0_u32;
            let mut tokens = 0_u64;
            q.read(&name, &mut mask, &mut tokens);
            if !q.decided(mask) {
                q.read(&keyword, &mut mask, &mut tokens);
            }
            assert_eq!(q.decided(mask), expected, "name {name:?}, words {keyword:?}, asked {asked:?}, all {all}");
            found += usize::from(expected);
        }
    }
    assert!(found > 200, "the cases hold matches: {found}");
}

fn named(id: u32, name: &str, keywords: Option<&str>) -> Arc<diacraft_doc::Master> {
    let mut sheet = Sheet::default();
    if let Some(k) = keywords {
        sheet.cells.insert(CellName::new("ShapeKeywords"), Cell::constant(Value::from_text(k)));
    }
    Arc::new(diacraft_doc::Master { id, name: Some(Arc::from(name)), sheet: Arc::new(sheet), ..diacraft_doc::Master::default() })
}

fn stencil_of(masters: Vec<Arc<diacraft_doc::Master>>) -> OpenedStencils {
    let mut doc = Document::new_blank();
    for m in masters {
        doc.masters.push(m);
    }
    let mut opened = OpenedStencils::new();
    let id = opened.next_id();
    opened.add(OpenedStencil::new(id, "Big".to_owned(), None, 0, ModelCost::default(), doc));
    opened
}

/// A stencil of 120,000 masters: the search looks at 100,000 of them (the built-in ones among them), stops, and says how far it got.
#[test]
fn a_stencil_of_more_masters_than_the_search_takes_is_read_to_the_limit_and_the_answer_says_so() {
    let opened = stencil_of((1..=120_000).map(|i| named(i, &format!("Pump {i}"), None)).collect());
    let outcome = search(&opened, &query("nothing"));
    assert_eq!(outcome.searched, MAX_SEARCHED_MASTERS);
    assert_eq!(outcome.stopped, Some(Stop::Masters));
    let built_in = crate::stencil::all_masters().count();
    assert_eq!(outcome.total, 120_000 + built_in);
    assert_eq!(outcome.matches, 0);
    let v = answer("nothing", &query("nothing"), &outcome);
    assert_eq!(v["complete"], false);
    let notes = v["notes"].as_array().unwrap();
    assert!(notes[0].as_str().unwrap().contains(&format!("looked at 100000 of {} masters and stopped", 120_000 + built_in)), "{notes:?}");
    // The master past the limit is not found, and one inside it is: the limit is where the answer says it is.
    assert_eq!(search(&opened, &query("119999")).matches, 0, "the master after the limit was not read");
    let inside = search(&opened, &query("1000"));
    assert_eq!(inside.matches, 1);
    assert_eq!(inside.groups[0].masters[0]["name"], "Pump 1000");
}

/// Names and search words of megabytes: a name is cut when the stencil is opened, a search word text is read to 4,096 bytes and no further, and what is read is charged first.
#[test]
fn names_and_search_words_of_megabytes_cost_what_they_are_charged_and_no_more() {
    let long = "w ".repeat(600_000);
    let tail = format!("{}needle", "x ".repeat(2_500));
    let masters: Vec<_> = (1..=12)
        .map(|i| {
            let words = if i == 1 { tail.as_str() } else { long.as_str() };
            named(i, &format!("{}{i}", "n".repeat(1_500_000)), Some(words))
        })
        .collect();
    let opened = stencil_of(masters);
    let outcome = search(&opened, &query("zzz"));
    let built_in = crate::stencil::all_masters().count();
    // Twelve masters of at most 128 characters of name and 4,096 bytes of words each (the built-in ones come first and are small).
    assert!(outcome.stopped.is_none() && outcome.searched == 12 + built_in, "{outcome:?}");
    assert!(outcome.bytes <= 12 * (140 + MAX_KEYWORD_BYTES) + 20_000, "charged {} bytes", outcome.bytes);
    assert_eq!(outcome.keywords_cut, 12);
    assert!(
        outcome.tokens <= outcome.bytes as u64,
        "each word costs a byte of what was charged, at least: {} words for {} bytes",
        outcome.tokens,
        outcome.bytes
    );
    // The words past 4,096 bytes are not read: the needle after 5,000 bytes is not found, one at the start is.
    assert_eq!(search(&opened, &query("needle")).matches, 0);
    let near = stencil_of(vec![named(1, "Near", Some(&format!("needle {}", "x ".repeat(5_000))))]);
    assert_eq!(search(&near, &query("needle")).matches, 1);
    let notes = answer("zzz", &query("zzz"), &outcome)["notes"].clone();
    assert!(notes[0].as_str().unwrap().contains("12 masters have search words longer than 4096 bytes"), "{notes}");
}

/// Enough text to pass 16 MiB: the search stops by its byte budget, and the budget is not passed by what it read.
#[test]
fn a_search_that_runs_out_of_bytes_stops_and_never_passes_its_budget() {
    let text = "k ".repeat(MAX_KEYWORD_BYTES / 2);
    let opened = stencil_of((1..=6_000).map(|i| named(i, &format!("M{i}"), Some(&text))).collect());
    let outcome = search(&opened, &query("zzz"));
    assert_eq!(outcome.stopped, Some(Stop::Bytes), "{outcome:?}");
    assert!(outcome.bytes <= MAX_SEARCHED_BYTES, "{}", outcome.bytes);
    assert!(outcome.searched < outcome.total);
    // Each master read costs its name and 4,096 bytes of words: the stop comes at the master that would pass the budget.
    let most = MAX_SEARCHED_BYTES / MAX_KEYWORD_BYTES;
    assert!(outcome.searched <= most + crate::stencil::all_masters().count(), "{}", outcome.searched);
    assert!(outcome.searched + 8 >= most, "{}: the budget was used", outcome.searched);
    assert!(answer("zzz", &query("zzz"), &outcome)["notes"][0].as_str().unwrap().contains("16 MiB"));
}

/// The words of a master with many shapes: only the first few shapes' cells are read, the page sheet's always.
#[test]
fn only_the_first_shapes_of_a_master_have_their_search_words_read() {
    let sheet_with = |word: &str| {
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("ShapeKeywords"), Cell::constant(Value::from_text(word)));
        Arc::new(sheet)
    };
    let mut master = diacraft_doc::Master { id: 1, name: Some(Arc::from("Many")), ..diacraft_doc::Master::default() };
    master.sheet = sheet_with("page");
    master.shapes = (0..20_u32)
        .map(|k| {
            let mut shape = diacraft_doc::Shape::new(k + 1);
            shape.sheet = sheet_with(&format!("shape{k}"));
            Arc::new(shape)
        })
        .collect();
    let texts = keyword_texts(&master);
    assert_eq!(texts.len(), 1 + KEYWORD_SHAPES);
    assert_eq!(texts[0], "page");
    assert_eq!(texts[KEYWORD_SHAPES], format!("shape{}", KEYWORD_SHAPES - 1));
}

#[test]
fn a_cut_is_at_a_character() {
    assert_eq!(cut("abcdef", 3), "abc");
    assert_eq!(cut("éé", 3), "é", "a character is not cut in two");
    assert_eq!(cut("é", 1), "");
    assert_eq!(cut("abc", 10), "abc");
}

/// A master of thousands of deleted shapes is looked at in its first 64 only, so its search words cost what that costs.
#[test]
fn a_master_of_deleted_shapes_is_scanned_for_search_words_in_its_first_shapes_only() {
    let sheet_with = |word: &str| {
        let mut sheet = Sheet::default();
        sheet.cells.insert(CellName::new("ShapeKeywords"), Cell::constant(Value::from_text(word)));
        Arc::new(sheet)
    };
    let mut master = diacraft_doc::Master { id: 1, name: Some(Arc::from("Deleted")), ..diacraft_doc::Master::default() };
    master.shapes = (0..10_000_u32)
        .map(|k| {
            let mut shape = diacraft_doc::Shape::new(k + 1);
            shape.del = k < 9_990;
            shape.sheet = sheet_with(&format!("shape{k}"));
            Arc::new(shape)
        })
        .collect();
    assert!(keyword_texts(&master).is_empty(), "the live shapes are past the first 64: they are not looked at");
}
