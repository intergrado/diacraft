//! The costs of what it holds and writes (of the 0.3 plan): crafted inputs beside a yardstick, each held to a count of the units its loops walk (elements, style
//! steps, items, the steps of a save), never to a time; the real-time check is an `#[ignore]` test that times the input back to back with the yardstick, and the refusal that must not
//! abort runs in a child process under `ulimit -v 3000000`.
//!
//! **Measured on beast on 2026-11-25** (the test profile): a style no shape names costs the read 3 elements, 1 style step and 23 items, and the model save of it 9 steps; one with no name
//! costs 2 elements, no style step, 17 items and 8 steps. The yardstick (the model save of a drawing of 20,000 shapes on 1,000 automatic styles) is the figure every input is compared to.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use diacraft_doc::{KeptStyle, RawAttrs, RawElement};
use diacraft_odg::{LimitKind, Limits, OdgError, WriteError, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{rect, source_of, through_codec};

const SCENARIO: &str = "ODG_VISIT_STYLES_SCENARIO";

fn unused(n: usize) -> String {
    (0..n)
        .map(|k| format!(r##"<style:style style:name="u{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##))
        .collect()
}

fn nameless(n: usize) -> String {
    r##"<style:style style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00"/></style:style>"##.repeat(n)
}

/// The yardstick: a drawing of `shapes` rectangles on `styles` automatic styles, none unused.
fn yardstick(shapes: usize, styles: usize) -> Drawing {
    let defs: String = (0..styles)
        .map(|k| format!(r##"<style:style style:name="g{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##))
        .collect();
    let page: String = (0..shapes).map(|k| rect(&format!("g{}", k % styles.max(1)))).collect();
    Drawing::a4().auto(&defs).page(&page)
}

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

/// A style nothing names costs the read and the save a constant, and four times as many cost four times as much.
#[test]
fn styles_nothing_names_cost_a_constant_each() {
    let none = counts(&Drawing::a4().page(&rect("")));
    let (small, large) = (counts(&Drawing::a4().auto(&unused(1_000)).page(&rect(""))), counts(&Drawing::a4().auto(&unused(4_000)).page(&rect(""))));
    for (n, c) in [(1_000u64, &small), (4_000, &large)] {
        assert!(c.elements - none.elements <= 3 * n + 4, "{n}: the elements: {c:?}");
        assert!(c.style_steps - none.style_steps <= n, "{n}: the style steps: {c:?}");
        assert!(c.items - none.items <= 24 * n, "{n}: the items: {c:?}");
        assert!(c.steps - none.steps <= 10 * n, "{n}: the steps of the save: {c:?}");
        // The style is held whole and read once: no value of it is parsed for a use, and none is made for a shape.
        assert!(c.value_parses - none.value_parses <= 2 * n, "{n}: the values parsed: {c:?}");
    }
    assert!(
        (large.steps - none.steps) <= 4 * (small.steps - none.steps) + 100,
        "four times the styles cost four times the steps: {small:?} {large:?}"
    );
    assert!((large.items - none.items) <= 4 * (small.items - none.items) + 100, "{small:?} {large:?}");
}

/// 65,536 styles with no name, each held whole when it is read and written once.
#[test]
fn sixty_five_thousand_five_hundred_and_thirty_six_styles_with_no_name_cost_a_constant_each() {
    const N: u64 = 65_536;
    let none = counts(&Drawing::a4().page(&rect("")));
    let c = counts(&Drawing::a4().auto(&nameless(N as usize)).page(&rect("")));
    assert!(c.elements - none.elements <= 2 * N + 4, "{c:?}");
    assert_eq!(c.style_steps, none.style_steps, "a style with no name is named by nothing and is no step");
    assert!(c.items - none.items <= 18 * N, "{c:?}");
    assert!(c.steps - none.steps <= 9 * N, "{c:?}");
}

/// The repeats of one name are given names that no style of the file holds, each number tried once for all the repeats of a name (the cursor only grows): the steps of the read are
/// those of the styles and the repeats, and not their product.
#[test]
fn the_repeats_of_a_name_are_given_free_names_in_steps_of_their_own_number() {
    const REPEATS: usize = 3_000;
    const HELD: usize = 500;
    // `x` itself, `x_2` to `x_501` (the numbers a repeat of `x` would try first), and 3,000 repeats of `x`.
    let one = |name: &str, fill: &str| {
        format!(
            r##"<style:style style:name="{name}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="{fill}"/></style:style>"##
        )
    };
    let mut styles = one("x", "#111111");
    for k in 2..2 + HELD {
        styles.push_str(&one(&format!("x_{k}"), "#222222"));
    }
    styles.push_str(&one("x", "#333333").repeat(REPEATS));
    let drawing = Drawing::a4().auto(&styles).page(&rect("x"));
    let source = source_of(&drawing);
    let (doc, report) = read(&source, &Limits::default()).unwrap();
    let total = (1 + HELD + REPEATS) as u64;
    // One resolve step for each style, and one try for each repeat and for each number the file already holds: never the product.
    assert!(report.work.style_steps <= total + (REPEATS + HELD) as u64 + 8, "{} steps for {total} styles", report.work.style_steps);
    let kept: Vec<String> = doc
        .ext
        .get()
        .map(|d| d.odf.styles.iter().filter(|s| s.family.starts_with("unused")).map(|s| s.name.to_string()).collect())
        .unwrap_or_default();
    assert_eq!(kept.len(), HELD + REPEATS, "every style a shape does not name is held");
    let mut names = kept.clone();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), kept.len(), "each under a name of its own");
    assert!(!kept.iter().any(|n| n == "x"), "none under the name of the one a reference finds");
    let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let again = read(&written.bytes, &Limits::default()).unwrap().0;
    assert_eq!(again.ext.get().map(|d| d.odf.styles.len()), doc.ext.get().map(|d| d.odf.styles.len()), "a second read holds as many");
}

/// A chain of 64 automatic styles each the parent of the next (the standard forbids one and the reader follows it), the last with no name: the styles are resolved once, and a
/// thousand shapes on the first walk it no more than one does.
#[test]
fn a_chain_of_sixty_four_styles_is_no_walk_for_each_use() {
    let chain = |shapes: usize| {
        let mut styles: String =
            (0..63).map(|k| format!(r#"<style:style style:name="c{k}" style:family="graphic" style:parent-style-name="c{}"/>"#, k + 1)).collect();
        styles.push_str(r##"<style:style style:name="c63" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#336699"/></style:style>"##);
        styles.push_str(r#"<style:style style:family="graphic" style:parent-style-name="c0"/>"#);
        Drawing::a4().auto(&styles).page(&rect("c0").repeat(shapes))
    };
    let (one, many) = (counts(&chain(1)), counts(&chain(1_000)));
    assert!(many.style_steps <= one.style_steps + 1_000, "the chain is resolved once, not for each shape: {one:?} {many:?}");
    assert!(one.style_steps <= 4 * 65, "{one:?}");
}

/// An attribute of 1 MiB that no field reads, on styles nothing names: charged when it is read for what it costs in items, once for each style, and written once each.
#[test]
fn a_mebibyte_attribute_no_field_reads_is_charged_once_for_each_style() {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut noise = |len: usize| -> String {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                char::from(b'a' + ((seed >> 33) % 26) as u8)
            })
            .collect()
    };
    const MIB: usize = 1 << 20;
    let none = counts(&Drawing::a4().page(&rect("")));
    let four: String = (0..4).map(|k| format!(r#"<style:style style:name="m{k}" style:family="graphic" style:class="{}"/>"#, noise(MIB))).collect();
    let c = counts(&Drawing::a4().auto(&four).page(&rect("")));
    // A mebibyte of text is 18,432 items (an eighth more, at 64 bytes an item). Each style is charged it twice, by what stands at the peak: the remainder the style was given when it was read
    // (its own attribute, which no cell reads) and the whole style that takes its place once no shape is found to name it (`StyleSet::hold_unused`); once for the model, never for a use.
    let per_style = (c.items - none.items) / 4;
    assert!((2 * 18_400..2 * 18_500).contains(&per_style), "{per_style} items for each: {c:?}");
    assert!(c.value_parses - none.value_parses <= 8, "the values are not parsed: {c:?}");
    assert!(c.steps - none.steps <= 4 * 12, "the save walks each once: {c:?}");
}

/// The cap in words: styles past the items the reader may hold are refused when they are read, with the class and the words of the cap, and nothing is made.
#[test]
fn styles_nothing_names_past_the_item_cap_are_refused_in_words() {
    let source = source_of(&Drawing::a4().auto(&unused(1_000)).page(&rect("")));
    let limits = Limits { max_items: 5_000, ..Limits::default() };
    match read(&source, &limits) {
        Err(OdgError::Limit { kind: LimitKind::Items, max }) => assert_eq!(max, 5_000),
        other => panic!("expected the item cap, got {:?}", other.map(|(_, r)| r.work.items)),
    }
    // Under the cap it reads.
    assert!(read(&source, &Limits { max_items: 1_000_000, ..Limits::default() }).is_ok());
}

/// The model save refuses in words the unused styles past what a part of the package may hold, before any is copied.
#[test]
fn unused_styles_past_what_a_part_may_hold_are_refused_in_words_by_the_save() {
    let doc = kept_unused(20, 1 << 20);
    match write_with(&doc, None, &WriteOptions::default()) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("larger than the reader reads of a part"), "{words}"),
        other => panic!("expected a refusal in words, got {:?}", other.map(|w| w.bytes.len())),
    }
}

/// A blank model that holds `count` automatic styles nothing names, each with an attribute of `bytes` bytes (one text, shared by all of them).
fn kept_unused(count: usize, bytes: usize) -> diacraft_doc::Document {
    let mut doc = crate::model_write::blank(1);
    let text = "x".repeat(bytes);
    let shared = RawAttrs::from_pairs([("style:name", "u"), ("style:family", "graphic"), ("style:class", text.as_str())]);
    let odf = &mut doc.ext.make_mut().odf;
    for k in 0..count {
        odf.styles.push(KeptStyle {
            family: Arc::from("unused:graphic"),
            name: Arc::from(format!("u{k}")),
            element: Arc::new(RawElement { name: Arc::from("style:style"), attrs: shared.clone(), children: Vec::new() }),
        });
    }
    doc
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    match scenario.as_str() {
        "three_thousand_unused_styles_of_a_mebibyte" => {
            // Each fits the cap of the document's data alone, and three thousand of them are three gigabytes of markup: a save that copied them aborted. It is refused in words once they
            // pass the cap, and none past it is copied.
            let doc = kept_unused(3_000, 1 << 20);
            match write_with(&doc, None, &WriteOptions::default()) {
                Err(WriteError::Unsupported(words)) => assert!(words.contains("larger than the reader reads of a part"), "{words}"),
                other => panic!("expected a refusal in words, got {:?}", other.map(|w| w.bytes.len())),
            }
        }
        other => panic!("no scenario {other}"),
    }
}

fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact visit_styles_cost::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// A refusal is not an abort.
#[test]
fn three_thousand_unused_styles_of_a_mebibyte_are_refused_in_words_and_do_not_abort() {
    let (ended_well, stdout) = run_child("three_thousand_unused_styles_of_a_mebibyte", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// D1, the real-time check (run by hand): 100,000 styles nothing names are read, held and saved in no more than five times what the yardstick takes (it holds a fifth of the styles and shapes
/// that cost as much), and twice the styles take no more than two and a half times as long.
#[test]
#[ignore = "a timing: run by hand with --nocapture"]
fn a_hundred_thousand_unused_styles_take_no_more_than_five_times_the_yardstick() {
    let time = |drawing: &Drawing| {
        let start = Instant::now();
        let c = counts(drawing);
        (start.elapsed(), c)
    };
    let (base, c0) = time(&yardstick(20_000, 1_000));
    let (half, _) = time(&Drawing::a4().auto(&unused(50_000)).page(&rect("")));
    let (many, c1) = time(&Drawing::a4().auto(&unused(100_000)).page(&rect("")));
    println!("TIMING yardstick {base:?} ({c0:?}), 50,000 unused styles {half:?}, 100,000 {many:?} ({c1:?})");
    assert!(many <= base * 5, "{many:?} against {base:?}");
    assert!(many.as_secs_f64() <= half.as_secs_f64() * 2.5, "twice the styles: {many:?} against {half:?}");
}
