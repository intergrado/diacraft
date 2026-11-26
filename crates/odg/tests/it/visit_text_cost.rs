//! The costs of what it holds and writes (of the 0.3 plan): crafted inputs beside a yardstick, each held to a count of the units its loops walk (elements, style
//! steps, items, the steps of a save), never to a time; the real-time check is an `#[ignore]` test that times the input back to back with the yardstick, and the input that must be
//! refused, not aborted on, runs in a child process under `ulimit -v 3000000`.
//!
//! The yardstick is the model save of 2,000 paragraphs of 50 spans each (what the text writer walks in the common case). **Measured on beast on 2026-11-26** (the test profile): the yardstick reads
//! in 102,028 elements, 1,000,005 style steps and 1,004,803 items and saves in 1,118,596 steps; a span costs the read one element, ten style steps and ten items, and the save eleven steps. On
//! the base commit the same drawing took 904,797 items: the values a run's style holds (what it stated, [`diacraft_odg`]'s `read/spelled.rs`) are the difference, one item for each.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::time::Instant;

use diacraft_odg::{Limits, WriteOptions, read, write_with};
use diacraft_sheetxml::model::{MemBlobs, encode};

use crate::drawing::Drawing;
use crate::visit_styles::{assert_same, part, source_of, through_codec};

const SCENARIO: &str = "ODG_VISIT_TEXT_SCENARIO";

fn text_box(paragraphs: &str) -> String {
    format!(r#"<draw:frame svg:x="1cm" svg:y="1cm" svg:width="5cm" svg:height="2cm"><draw:text-box>{paragraphs}</draw:text-box></draw:frame>"#)
}

fn span(style: &str, text: &str) -> String {
    format!(r#"<text:span text:style-name="{style}">{text}</text:span>"#)
}

const BOLD: &str = r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#;
const LANGUAGE: &str = r#"<style:style style:name="T2" style:family="text"><style:text-properties fo:language="en"/></style:style>"#;

/// What a read and the model save of a drawing counted.
#[derive(Debug)]
struct Counts {
    elements: u64,
    style_steps: u64,
    value_parses: u64,
    items: u64,
    steps: u64,
}

fn counts(drawing: &Drawing) -> Counts {
    let source = source_of(drawing);
    let (doc, report) = read(&source, &Limits::default()).unwrap();
    let doc = through_codec(&doc);
    let written = write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    Counts {
        elements: report.work.elements,
        style_steps: report.work.style_steps,
        value_parses: report.work.value_parses,
        items: report.work.items,
        steps: written.work.steps_used(),
    }
}

/// `paragraphs` paragraphs of `spans` spans each of the text `text`, alternating the styles `T1` and `T2`.
fn spans_of(paragraphs: usize, spans: usize, text: &str) -> Drawing {
    let para: String = format!("<text:p>{}</text:p>", (0..spans).map(|k| span(if k % 2 == 0 { "T1" } else { "T2" }, text)).collect::<String>());
    Drawing::a4().auto(BOLD).auto(LANGUAGE).page(&text_box(&para.repeat(paragraphs)))
}

/// A paragraph style 64 levels deep (each the parent of the one before, each holding text properties), and `paragraphs` paragraphs on the first.
fn deep(paragraphs: usize) -> Drawing {
    let styles: String = (0..64)
        .map(|k| {
            let parent = if k < 63 { format!(r#" style:parent-style-name="D{}""#, k + 1) } else { String::new() };
            format!(
                r#"<style:style style:name="D{k}" style:family="paragraph"{parent}><style:paragraph-properties fo:margin-left="{k}mm"/><style:text-properties fo:font-size="{}pt"/></style:style>"#,
                10 + k
            )
        })
        .collect();
    let para = r#"<text:p text:style-name="D0">Hello</text:p>"#.repeat(paragraphs);
    Drawing::a4().auto(&styles).page(&text_box(&para))
}

/// `paragraphs` paragraphs of a shape whose style gives its text a size, so that each paragraph's text is on a row that holds cells.
fn on_a_sized_shape(paragraphs: usize) -> Drawing {
    let style = r##"<style:style style:name="gr1" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/><style:text-properties fo:font-size="24pt"/></style:style>"##;
    let para = "<text:p>word</text:p>".repeat(paragraphs);
    Drawing::a4()
        .auto(style)
        .page(&format!(r#"<draw:rect draw:style-name="gr1" svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{para}</draw:rect>"#))
}

/// 100,000 spans of two styles in one paragraph cost the read and the save what the yardstick's 100,000 in 2,000 paragraphs do, and a constant for each span.
#[test]
fn a_hundred_thousand_spans_in_one_paragraph_cost_what_the_yardstick_does() {
    let yard = counts(&spans_of(2_000, 50, "word "));
    let one = counts(&spans_of(1, 100_000, "word "));
    for (what, a, b) in [
        ("elements", one.elements, yard.elements),
        ("style steps", one.style_steps, yard.style_steps),
        ("items", one.items, yard.items),
        ("steps", one.steps, yard.steps),
    ] {
        assert!(a <= b + b / 20, "{what}: {a} against the yardstick's {b}: {one:?} {yard:?}");
    }
    assert!(one.items <= 11 * 100_000, "an item for each span, and ten for what it reads: {one:?}");
    assert!(one.steps <= 12 * 100_000, "{one:?}");
    // The values of a style are made once for each row it is used with: a use parses no value.
    assert_eq!(one.value_parses, yard.value_parses, "{one:?} {yard:?}");
}

/// One character in each of the spans, as the same text in one span would be.
#[test]
fn a_hundred_thousand_spans_of_one_character_cost_what_the_same_spans_of_a_word_do() {
    let word = counts(&spans_of(1, 100_000, "word "));
    let one = counts(&spans_of(1, 100_000, "x"));
    assert!(one.steps <= word.steps + word.steps / 20, "{one:?} {word:?}");
    assert!(one.items <= word.items + word.items / 20, "{one:?} {word:?}");
}

/// The uses of a style add a constant, whatever their number (the values it holds are looked at once for each row, `Spelling`).
#[test]
fn the_uses_of_a_style_are_a_constant_each_and_the_style_is_looked_at_once() {
    let (small, large) = (counts(&spans_of(1, 1_000, "w")), counts(&spans_of(1, 100_000, "w")));
    assert!(large.items <= 100 * small.items + 1_000, "a hundred times the spans, a hundred times the items: {small:?} {large:?}");
    assert!(large.steps <= 100 * small.steps + 1_000, "{small:?} {large:?}");
    assert_eq!(small.value_parses, large.value_parses, "no value is parsed for a use: {small:?} {large:?}");
}

/// A paragraph style 64 levels deep, each level holding text properties and the next as its parent, used by 1 paragraph and by 10,000: the chain is read once, and each use costs what a
/// paragraph that holds a held value costs.
#[test]
fn a_paragraph_style_sixty_four_levels_deep_is_read_once_and_each_use_costs_a_constant() {
    let (one, many) = (counts(&deep(1)), counts(&deep(10_000)));
    assert_eq!(one.value_parses, many.value_parses, "the chain's values are parsed once: {one:?} {many:?}");
    assert!(one.value_parses <= 2 * 64, "{one:?}");
    assert!(many.style_steps <= one.style_steps + 13 * 10_000, "{one:?} {many:?}");
    assert!(many.items <= one.items + 5 * 10_000, "{one:?} {many:?}");
    assert!(many.steps <= one.steps + 11 * 10_000, "{one:?} {many:?}");
}

/// A paragraph whose text is on a row that holds cells is given a value that says so (`Spelling::paragraph`): 100,000 of them cost the read a few items each.
#[test]
fn paragraphs_on_a_row_that_holds_cells_cost_a_few_items_each() {
    let (one, many) = (counts(&on_a_sized_shape(1)), counts(&on_a_sized_shape(100_000)));
    assert!(many.items <= one.items + 12 * 100_000, "{one:?} {many:?}");
    assert!(many.steps <= one.steps + 12 * 100_000, "{one:?} {many:?}");
    assert_eq!(one.value_parses, many.value_parses, "{one:?} {many:?}");
}

/// A mebibyte of text in one span is charged by its bytes and written once.
#[test]
fn a_mebibyte_of_text_in_one_span_costs_its_bytes_and_a_constant() {
    let mib = "x".repeat(1 << 20);
    let drawing = Drawing::a4().auto(BOLD).page(&text_box(&format!("<text:p>{}</text:p>", span("T1", &mib))));
    let c = counts(&drawing);
    assert!(c.elements <= 40, "{c:?}");
    assert!(c.items <= 20_000, "an eighth more than a mebibyte at 64 bytes an item: {c:?}");
    assert!(c.steps <= 18_000, "{c:?}");
    // And it is the same content.
    let written = assert_same("a mebibyte in a span", &drawing);
    let content = part(&written, "content.xml");
    assert!(content.len() >= 1 << 20 && content.contains(r#"<text:span text:style-name="T1">xxxx"#));
}

/// D1, the cap the whole-model codec holds in words: a text of a million spans is refused when the model is encoded, as the model of the base commit was, and is no abort.
#[test]
fn a_million_spans_are_refused_by_the_codec_in_words() {
    match encoded_refusal() {
        Some(words) => assert!(words.contains("XML items in the parts held at once"), "{words}"),
        None => panic!("a million spans were encoded"),
    }
}

/// The words of the codec's refusal of the model of a million one-character spans, `None` when it encodes.
fn encoded_refusal() -> Option<String> {
    let drawing = spans_of(1, 1 << 20, "x");
    let (doc, _) = read(&source_of(&drawing), &Limits::default()).unwrap();
    let mut blobs = MemBlobs::new();
    encode(&doc, &mut blobs).err().map(|e| format!("{e:?}"))
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the test below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    match scenario.as_str() {
        "a_million_one_character_spans" => {
            let words = encoded_refusal().expect("the codec refuses a million spans");
            assert!(words.contains("XML items in the parts held at once"), "{words}");
        }
        other => panic!("no scenario {other}"),
    }
}

fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact visit_text_cost::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// A refusal is not an abort.
#[test]
fn a_million_one_character_spans_are_refused_in_words_and_do_not_abort() {
    let (ended_well, stdout) = run_child("a_million_one_character_spans", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// D1, the real-time check (run by hand): 100,000 spans in one paragraph are read, held and saved in no more than twice the time of the yardstick's 100,000 in 2,000 paragraphs.
#[test]
#[ignore = "a timing: run by hand with --nocapture"]
fn a_hundred_thousand_spans_take_no_more_than_twice_the_yardstick() {
    let time = |drawing: &Drawing| {
        let start = Instant::now();
        let c = counts(drawing);
        (start.elapsed(), c)
    };
    let (base, c0) = time(&spans_of(2_000, 50, "word "));
    let (many, c1) = time(&spans_of(1, 100_000, "word "));
    println!("TIMING yardstick {base:?} ({c0:?}), one paragraph {many:?} ({c1:?})");
    assert!(many <= base * 2, "{many:?} against {base:?}");
}
