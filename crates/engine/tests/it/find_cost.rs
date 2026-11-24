//! What a search costs on a hostile drawing. A search over 76,000 shapes, over a text of 10 MB and over a pattern and a text made to be slow is **charged** (a
//! step for each shape, character and row read, counted in the answer of the engine's own function, never timed), **bounded** (a budget of steps past which the search is
//! refused in words, and a cap on the matches a listing returns or a Replace All replaces) and, in the ignored test, **timed against a yardstick**: the same walk of the same
//! drawing done by hand, and `str::matches` over the same text. Run the timed one by hand, alone: `cargo test -p diacraft-engine --test it -- --ignored find_cost --nocapture
//! --test-threads=1`.

use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{Document, RowKey, TextPart};
use diacraft_engine::draw::find::{self, Pattern, Scope, Within};
use diacraft_engine::{EngineError, Session};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{check_session, run, snapshot};

/// 76,000 shapes over three pages (the most a drawing may hold, `vsdx_save_peak.rs`), each with a label `Label <n>`.
fn big_drawing(with_data: bool) -> Document {
    let n = 76_000_u32;
    let per = n.div_ceil(3);
    let mut builder = DocBuilder::new();
    if with_data {
        builder = builder.master(
            1,
            MasterBuilder::new().shape(
                ShapeBuilder::new(1)
                    .row_cell("Property", None, RowKey::name("Owner"), "Value", "Nobody")
                    .row_cell("Property", None, RowKey::name("Site"), "Value", "Somewhere")
                    .row_cell("Property", None, RowKey::name("Cost"), "Value", "Free"),
            ),
        );
    }
    for p in 0..3_u32 {
        let mut page = PageBuilder::new();
        for i in (p * per)..((p + 1) * per).min(n) {
            let mut shape = ShapeBuilder::new(i + 1).text(&format!("Label {i}"));
            if with_data {
                shape = shape.master(1);
            }
            page = page.shape(shape);
        }
        builder = builder.page(p + 1, page);
    }
    builder.build()
}

fn session_of(doc: Document) -> Session {
    let mut s = crate::blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

fn pattern(what: &str, case: bool) -> Pattern {
    Pattern::parse(what, case, false).unwrap()
}

/// **Charged.** A search of 76,000 shapes visits each once and charges a few steps for it: the steps are the shapes' and their labels', and nothing grows with the pages.
#[test]
fn a_search_over_76000_shapes_is_charged_a_few_steps_for_each() {
    let doc = big_drawing(false);
    let found = find::find(&doc, &Scope::All, &pattern("label 7", false), Within::TEXT, None, 100, false).unwrap();
    assert_eq!(found.found.len(), 100, "a list cut at its limit");
    assert!(found.truncated);
    // The search stops when the list is full: the first hundred matches are in the first shapes... `Label 7` is shape 8, `Label 70` to `Label 79` are shapes 71 to 80.
    assert!(found.steps < 100_000, "stopped early: {} steps", found.steps);
    // A search with no match reads every shape: eight steps to pass it and a step for each character of a label of about ten.
    let none = find::find(&doc, &Scope::All, &pattern("nothing like this", false), Within::TEXT, None, 100, false).unwrap();
    assert_eq!((none.shapes, none.found.len(), none.truncated), (76_000, 0, false));
    let (low, high) = (76_000 * 8, 76_000 * (8 + 4 + 12));
    assert!((low..=high).contains(&none.steps), "{} steps for 76,000 shapes: expected {low} to {high}", none.steps);
    // The plain case is cheaper to charge, and the same shapes: a step for a byte.
    let literal = find::find(&doc, &Scope::All, &pattern("nothing like this", true), Within::TEXT, None, 100, false).unwrap();
    assert!(literal.steps <= none.steps, "{} against {}", literal.steps, none.steps);
}

/// **Charged: rows.** Shape data costs a row's steps for each row each shape has, master's rows included, and a search that asks for it reads each once.
#[test]
fn shape_data_is_charged_for_every_row_of_every_shape_including_the_masters() {
    let doc = big_drawing(true);
    let within = Within { name: false, text: false, data: true, user: false };
    let found = find::find(&doc, &Scope::Page(1), &pattern("nowhere", false), within, None, 100, false).unwrap();
    assert_eq!(found.shapes, 25_334);
    assert!(found.found.is_empty());
    // Three rows a shape, each charged `ROW_STEPS`: more than the shapes' own steps, and in proportion to the rows.
    let rows = 25_334_u64 * 3;
    assert!(found.steps >= rows * find::ROW_STEPS, "{} steps for {rows} rows", found.steps);
    assert!(found.steps <= rows * find::ROW_STEPS + 25_334 * 80, "{} steps for {rows} rows", found.steps);
    // And what the rows hold is found: the master's value on a shape that never wrote it.
    let some = find::find(&doc, &Scope::Page(1), &pattern("somewhere", false), within, None, 3, false).unwrap();
    assert_eq!(some.found.len(), 3);
    assert_eq!(some.found[0].spot.row.as_deref(), Some("Site"));
}

/// **Bounded.** Past its budget a search is refused in words and the drawing is as it was, whatever it was asked: here a text of 10 MB and a pattern whose every start is
/// nearly a match.
#[test]
fn a_pattern_and_a_text_made_to_be_slow_are_refused_in_words_at_the_budget() {
    let ten_mb = "a".repeat(10 * 1024 * 1024);
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text(&ten_mb))).build();
    let hostile = format!("{}b", "a".repeat(999));
    let refused = find::find(&doc, &Scope::All, &pattern(&hostile, false), Within::TEXT, None, 100, false).unwrap_err();
    assert!(!refused.bad_params, "not the caller's mistake: the drawing is too big for the pattern");
    assert!(refused.message.starts_with("the search is too large to finish"), "{}", refused.message);
    // As a command, and in a session: the error is the engine's, and nothing changed.
    let mut s = session_of(doc);
    let before = snapshot(&s);
    let error = s.execute("edit.find", &json!({"what": hostile})).unwrap_err();
    assert!(matches!(error, EngineError::Failed { .. }), "{error:?}");
    assert!(error.to_string().contains("too large to finish"), "{error}");
    assert_eq!(snapshot(&s), before);
}

/// **A text of 10 MB** is searched by the plain case at a step a byte, and the case-folding loop at about two steps a character, both within the budget, and every match is found.
#[test]
fn a_text_of_ten_megabytes_is_searched_within_the_budget_and_every_match_is_found() {
    let unit = "word ";
    let text = unit.repeat(2 * 1024 * 1024); // 10 MiB
    assert_eq!(text.len(), 10 * 1024 * 1024);
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text(&text))).build();
    let plain = find::find(&doc, &Scope::All, &pattern("word", true), Within::TEXT, None, 10_000, false).unwrap();
    assert_eq!((plain.found.len(), plain.truncated), (10_000, true));
    assert_eq!(plain.found[9_999].start, 9_999 * 5, "the matches are where `word ` repeats");
    let folded = find::find(&doc, &Scope::All, &pattern("WORD", false), Within::TEXT, None, 10_000, false).unwrap();
    assert_eq!(folded.found.len(), 10_000);
    // No match in 10 MB: the plain case charges the bytes, the folding case two steps a character, and both are far inside the budget.
    let none_plain = find::find(&doc, &Scope::All, &pattern("needle", true), Within::TEXT, None, 10, false).unwrap();
    assert!((10 * 1024 * 1024..=12 * 1024 * 1024).contains(&none_plain.steps), "{}", none_plain.steps);
    let none_folded = find::find(&doc, &Scope::All, &pattern("needle", false), Within::TEXT, None, 10, false).unwrap();
    assert!(none_folded.steps <= 3 * 10 * 1024 * 1024, "{}", none_folded.steps);
    assert!(none_folded.steps < find::MAX_FIND_STEPS);
    // A listing is never longer than the cap, whatever limit is asked.
    let capped = find::find(&doc, &Scope::All, &pattern("o", true), Within::TEXT, None, usize::MAX, false).unwrap();
    assert_eq!(capped.found.len(), find::MAX_LISTED);
    assert!(capped.truncated);
    // The contexts of ten thousand matches in one text are made in one pass over it, not one pass for each: counted in the time the listing takes by the steps it charged.
    assert!(capped.steps < 1_500_000, "{}", capped.steps);
}

/// **Replace All in a big text is one pass.** Fifty thousand matches in one text of 100,000 characters are replaced in the one rewrite of its parts: the steps are those of
/// the search, a few for each character, and not a rewrite for each match.
#[test]
fn replace_all_of_fifty_thousand_matches_in_one_text_is_one_pass() {
    let text = "a ".repeat(50_000);
    let doc =
        DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(vec![TextPart::Chars(Arc::from(text.as_str()))]))).build();
    let mut s = session_of(doc);
    let r = run(&mut s, "edit.replace", json!({"what": "a", "with": "bb", "scope": "all", "match_case": true}));
    // The text would be 150,000 characters: more than a shape's text may be, so it is left, in words.
    assert_eq!((r["replaced"].clone(), r["skipped_total"].clone()), (json!(0), json!(50_000)));
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("too long"));
    // Shorter is allowed, and is done at once.
    let r = run(&mut s, "edit.replace", json!({"what": "a", "with": "", "scope": "all", "match_case": true}));
    assert_eq!(r["replaced"], 50_000);
    let text = diacraft_engine::draw::text_view(s.document().unwrap(), 1, 1).unwrap().chars;
    assert_eq!(text, " ".repeat(50_000));
    assert_eq!(check_session(&s), Ok(()));
}

/// **More matches than a Replace All may make** are refused in words and change nothing.
#[test]
fn more_matches_than_a_replace_all_may_make_are_refused_in_words() {
    let text = "a".repeat(find::MAX_REPLACEMENTS + 1);
    let doc =
        DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text_parts(vec![TextPart::Chars(Arc::from(text.as_str()))]))).build();
    let mut s = session_of(doc);
    let before = snapshot(&s);
    let error = s.execute("edit.replace", &json!({"what": "a", "with": "", "scope": "all", "match_case": true})).unwrap_err();
    assert!(error.to_string().contains("more than 1000000 matches"), "{error}");
    assert_eq!(snapshot(&s), before);
}

/// **Hostile structure.** A group nested as deep as the model allows is walked, a shape with a thousand rows of shape data is read within its charge, and neither is
/// walked twice.
#[test]
fn a_deep_group_and_a_shape_with_many_rows_are_walked_once_and_charged() {
    let mut deepest = ShapeBuilder::new(200).text("needle");
    for id in (1..200).rev() {
        deepest = ShapeBuilder::new(id).group().child(deepest);
    }
    let mut data = ShapeBuilder::new(300);
    for i in 0..1_000 {
        data = data.row_cell("Property", None, RowKey::name(&format!("Row{i}")), "Value", &format!("value {i}"));
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(deepest).shape(data)).build();
    let within = Within { name: true, text: true, data: true, user: true };
    let found = find::find(&doc, &Scope::All, &pattern("needle", false), within, None, 10, false).unwrap();
    assert_eq!(found.found.len(), 1);
    assert_eq!(found.found[0].spot.id, 200);
    assert_eq!(found.shapes, 201, "every shape once");
    assert!(found.steps >= 1_000 * find::ROW_STEPS, "the thousand rows are charged: {}", found.steps);
    assert!(found.steps < 1_000 * find::ROW_STEPS + 100_000, "{}", found.steps);
}

/// **Timed against a yardstick** (ignored: a measurement, run alone). The yardstick of the shapes is a walk of the same drawing that sums the length of each label, and the
/// yardstick of the text is `str::matches` over it. The figures go to the output; the assertions are loose enough for a loaded machine and tight enough to catch a quadratic.
#[test]
#[ignore = "a measurement, not a test: run alone with --ignored --nocapture --test-threads=1"]
fn find_cost_timed_against_a_yardstick() {
    let doc = big_drawing(true);
    let walk_yardstick = || {
        let started = Instant::now();
        let mut total = 0_usize;
        for page in doc.pages.iter() {
            for shape in page.shapes.iter() {
                total += shape.text.as_ref().map_or(0, |t| t.parts.len()) + usize::from(shape.master.is_some());
            }
        }
        (started.elapsed(), total)
    };
    let (yard, total) = walk_yardstick();
    assert!(total > 0);
    let timed = |what: &str, case: bool, within: Within| {
        let started = Instant::now();
        let r = find::find(&doc, &Scope::All, &pattern(what, case), within, None, 10, false).unwrap();
        (started.elapsed(), r.steps, r.shapes)
    };
    let text = timed("no such label", false, Within::TEXT);
    let data = timed("no such value", false, Within { name: false, text: false, data: true, user: false });
    eprintln!("yardstick, a walk of 76,000 shapes: {yard:?}");
    eprintln!("find in text, 76,000 shapes: {:?}, {} steps, {} shapes", text.0, text.1, text.2);
    eprintln!("find in shape data, 76,000 shapes with 3 rows each: {:?}, {} steps, {} shapes", data.0, data.1, data.2);
    assert!(text.0 <= yard * 200 + std::time::Duration::from_millis(50), "text search against the walk");
    assert!(data.0 <= std::time::Duration::from_secs(10), "{:?}", data.0);

    let ten_mb = "word ".repeat(2 * 1024 * 1024);
    let one = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text(&ten_mb))).build();
    let started = Instant::now();
    let count = ten_mb.matches("needle").count();
    let str_yard = started.elapsed();
    let started = Instant::now();
    let plain = find::find(&one, &Scope::All, &pattern("needle", true), Within::TEXT, None, 10, false).unwrap();
    let plain_time = started.elapsed();
    let started = Instant::now();
    let folded = find::find(&one, &Scope::All, &pattern("needle", false), Within::TEXT, None, 10, false).unwrap();
    let folded_time = started.elapsed();
    eprintln!("yardstick, str::matches over 10 MB: {str_yard:?} ({count} matches)");
    eprintln!("find, plain, 10 MB: {plain_time:?}, {} steps", plain.steps);
    eprintln!("find, folding case, 10 MB: {folded_time:?}, {} steps", folded.steps);
    assert!(plain_time <= str_yard * 20 + std::time::Duration::from_millis(20), "plain search against str::matches");
    assert!(folded_time <= std::time::Duration::from_secs(2), "{folded_time:?}");

    // The worst of it, at the budget: the time a refusal takes.
    let hostile = format!("{}b", "a".repeat(999));
    let all_a = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text(&"a".repeat(10 * 1024 * 1024)))).build();
    let started = Instant::now();
    let refused = find::find(&all_a, &Scope::All, &pattern(&hostile, false), Within::TEXT, None, 10, false);
    let refusal = started.elapsed();
    eprintln!("a refusal at the budget ({} steps): {refusal:?}", find::MAX_FIND_STEPS);
    assert!(refused.is_err());
    assert!(refusal <= std::time::Duration::from_secs(5), "{refusal:?}");
}

// ------------------------------------------------------------------------------------------------------------------ the budget of the text a replacement writes

/// `n` shapes, each holding the one word `word`, over one page: as text, and as a shape data row.
fn word_shapes(n: u32, in_data: bool) -> Document {
    let mut page = PageBuilder::new();
    for i in 0..n {
        let shape = if in_data {
            ShapeBuilder::new(i + 1).row_cell("Property", None, RowKey::name("Note"), "Value", "word")
        } else {
            ShapeBuilder::new(i + 1).text("word")
        };
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn within_data() -> Within {
    Within { name: false, text: false, data: true, user: false }
}

/// **The text a replacement writes is charged before it is made, and summed over the drawing.** Each text is held to a text's longest (100,000 characters) alone; a Replace All of 700
/// shapes by 100,000 characters each is 70 million, which is more than the 64 MiB one replacement may write, and is refused in words with nothing changed. Just under it is done.
#[test]
fn a_replace_all_that_would_write_more_than_its_budget_is_refused_in_words_before_it_writes() {
    let with = "x".repeat(diacraft_engine::draw::MAX_TEXT_CHARS);
    for in_data in [false, true] {
        let within = if in_data { within_data() } else { Within::TEXT };
        let mut doc = word_shapes(700, in_data);
        let before = doc.clone();
        let refused = find::replace(&mut doc, &Scope::All, &pattern("word", false), &with, within, None).unwrap_err();
        assert!(!refused.bad_params, "{refused:?}");
        assert!(refused.message.contains("more than 64 MiB") && refused.message.contains("budget"), "{}", refused.message);
        assert_eq!(doc, before, "nothing was written (data: {in_data})");
        // 600 of them are 60 million characters, which fits.
        let mut doc = word_shapes(600, in_data);
        let done = find::replace(&mut doc, &Scope::All, &pattern("word", false), &with, within, None).unwrap();
        assert_eq!((done.replaced, done.shapes), (600, 600), "data: {in_data}");
    }
}

/// A replacement is at most a text's longest, whatever the scope: one more character is refused as a bad parameter.
#[test]
fn a_replacement_is_at_most_a_texts_longest() {
    let mut s = session_of(word_shapes(2, false));
    let too_long = "x".repeat(diacraft_engine::draw::MAX_TEXT_CHARS + 1);
    let error = s.execute("edit.replace", &json!({"what": "word", "with": too_long, "scope": "all"})).unwrap_err();
    assert!(matches!(error, EngineError::BadParams { .. }) && error.to_string().contains("at most 100000 characters"), "{error}");
    let ok = s.execute("edit.replace", &json!({"what": "word", "with": "x".repeat(diacraft_engine::draw::MAX_TEXT_CHARS), "scope": "all"})).unwrap();
    assert_eq!(ok["replaced"], 2);
}

/// Every match counts towards the cap of a Replace All, those that are left as well as those that are replaced: a drawing whose matches are all left is not a way round it.
#[test]
fn matches_that_are_left_count_towards_the_cap_of_matches() {
    let text = "a".repeat(find::MAX_REPLACEMENTS / 2 + 1);
    let page = PageBuilder::new()
        .shape(ShapeBuilder::new(1).text_parts(vec![TextPart::Chars(Arc::from(text.as_str()))]))
        .shape(ShapeBuilder::new(2).text_parts(vec![TextPart::Chars(Arc::from(text.as_str()))]));
    let mut doc = DocBuilder::new().page(1, page).build();
    // The text would be longer than a text may be, so every match is left: two shapes hold a million and two matches between them.
    let refused = find::replace(&mut doc, &Scope::All, &pattern("a", true), "bb", Within::TEXT, None).unwrap_err();
    assert!(refused.message.contains("more than 1000000 matches"), "{}", refused.message);
}

const BUDGET_CHILD: &str = "ENGINE_FIND_BUDGET";

/// The child of the tests below, run under `ulimit -v 3000000`: 76,000 shapes (or one long text) replaced as the scenario says, which must be refused in words or finish, never abort.
#[test]
#[ignore = "run by the tests below under `ulimit -v`: alone it needs the scenario in the environment"]
fn find_budget_child() {
    let scenario = std::env::var(BUDGET_CHILD).expect("the scenario");
    let with = "x".repeat(diacraft_engine::draw::MAX_TEXT_CHARS);
    match scenario.as_str() {
        "text" | "data" => {
            let mut s = session_of(word_shapes(76_000, scenario == "data"));
            let within = if scenario == "data" { json!(["shape_data"]) } else { json!(["text"]) };
            let error = s.execute("edit.replace", &json!({"what": "word", "with": with, "scope": "all", "in": within})).unwrap_err();
            assert!(error.to_string().contains("more than 64 MiB"), "refused in words: {error}");
            // The drawing is as it was, and a replacement that fits is done.
            let ok = run(&mut s, "edit.replace", json!({"what": "word", "with": "words", "scope": "all", "in": within}));
            assert_eq!(ok["replaced"], 76_000);
        }
        "long" => {
            // One text of the most characters a text may have, all of one letter: every character is a match.
            let long = "a".repeat(diacraft_engine::draw::MAX_TEXT_CHARS);
            let doc = DocBuilder::new().page(1, PageBuilder::new().shape(ShapeBuilder::new(1).text(&long))).build();
            let mut s = session_of(doc);
            // Replaced by a long text it would be 10,000 million characters: left, in words.
            let left = run(&mut s, "edit.replace", json!({"what": "a", "with": with, "scope": "all"}));
            assert_eq!((left["replaced"].clone(), left["skipped_total"].clone()), (json!(0), json!(100_000)));
            // Replaced by nothing it is done in one pass.
            let done = run(&mut s, "edit.replace", json!({"what": "a", "with": "", "scope": "all"}));
            assert_eq!(done["replaced"], 100_000);
        }
        other => panic!("no scenario {other}"),
    }
    println!("find budget: no abort");
}

#[cfg(target_os = "linux")]
fn run_budget_child(scenario: &str) {
    let exe = std::env::current_exe().unwrap();
    let script = "ulimit -v 3000000 && exec \"$0\" --exact find_cost::find_budget_child --ignored --nocapture --test-threads=1";
    let out = std::process::Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(BUDGET_CHILD, scenario).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("find budget: no abort"), "{scenario}: {text}");
}

/// 76,000 shapes that each hold the word, replaced by 100,000 characters each: 7.6 billion characters, which a process with 3 GB of address space cannot hold, is refused in words
/// at the plan and does not abort. (Without the sum over the drawing each text was held to 100,000 alone, and the write phase allocated them all.)
#[test]
#[cfg(target_os = "linux")]
fn seventy_six_thousand_shapes_replaced_by_a_long_text_are_refused_in_words_under_the_address_limit() {
    run_budget_child("text");
}

#[test]
#[cfg(target_os = "linux")]
fn seventy_six_thousand_shape_data_cells_replaced_by_a_long_text_are_refused_in_words_under_the_address_limit() {
    run_budget_child("data");
}

#[test]
#[cfg(target_os = "linux")]
fn one_text_of_the_most_characters_made_of_one_letter_matches_is_left_in_words_or_done_under_the_address_limit() {
    run_budget_child("long");
}

/// **A section of rows too long to read is told, not dropped.** A shape with more rows in one section than Find reads (20,000) is left unread, and the answer counts the sections.
#[test]
fn a_shape_with_more_rows_than_find_reads_is_told_in_the_answer_and_not_dropped_silently() {
    let mut data = ShapeBuilder::new(1);
    for i in 0..=find::MAX_ROWS_PER_SHAPE {
        data = data.row_cell("Property", None, RowKey::name(&format!("R{i}")), "Value", "needle");
    }
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(data)).build();
    let found = find::find(&doc, &Scope::All, &pattern("needle", false), within_data(), None, 10, false).unwrap();
    assert_eq!((found.found.len(), found.unread), (0, 1), "the rows are charged, none is read, and the section is counted");
    assert!(found.steps >= (find::MAX_ROWS_PER_SHAPE as u64) * find::ROW_STEPS);
    let mut s = session_of(doc);
    let r = run(&mut s, "edit.find", json!({"what": "needle", "scope": "all", "in": ["shape_data"]}));
    assert_eq!((r["matches"].as_array().unwrap().len(), r["unread"].clone()), (0, json!(1)));
    let r = run(&mut s, "edit.replace", json!({"what": "needle", "with": "pin", "scope": "all", "in": ["shape_data"]}));
    assert_eq!((r["replaced"].clone(), r["unread"].clone()), (json!(0), json!(1)));
}
