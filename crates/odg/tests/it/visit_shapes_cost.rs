//! The costs of what it writes back (of the 0.3 plan): crafted inputs beside a yardstick, each held to a count of the units its loops walk (elements, items, name
//! lookups, the steps of a save), never to a time; the real-time check is an `#[ignore]` test that times the input back to back with the yardstick, and the refusal that must not abort runs in
//! a child process under `ulimit -v 3000000`.
//!
//! The yardstick is the model save of a drawing of 20,000 rectangles and 10,000 connectors (the plan's); every input is compared to it, and to itself at a quarter of its size.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::time::Instant;

use diacraft_odg::{Limits, OdgError, WriteError, WriteOptions, read, write_with};

use crate::drawing::Drawing;
use crate::visit_styles::{GR1, source_of, through_codec};

const SCENARIO: &str = "ODG_VISIT_SHAPES_SCENARIO";

fn on_gr1(shapes: &str) -> Drawing {
    Drawing::a4().auto(GR1).page(shapes)
}

fn rect_with(attrs: &str, inner: &str) -> String {
    format!(r#"<draw:rect draw:style-name="gr1" {attrs} svg:x="2cm" svg:y="3cm" svg:width="4cm" svg:height="2cm">{inner}</draw:rect>"#)
}

fn connector_to(target: &str) -> String {
    format!(
        r#"<draw:connector draw:style-name="gr1" draw:start-shape="{target}" draw:start-glue-point="1" svg:x1="6cm" svg:y1="4cm" svg:x2="12cm" svg:y2="4cm"/>"#
    )
}

/// What a read and the model save of a drawing counted.
#[derive(Debug)]
struct Counts {
    elements: u64,
    items: u64,
    name_lookups: u64,
    steps: u64,
    bytes: usize,
}

fn counts(drawing: &Drawing) -> Counts {
    let source = source_of(drawing);
    let (doc, report) = read(&source, &Limits::default()).unwrap();
    let doc = through_codec(&doc);
    let written =
        write_with(&doc, None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap_or_else(|e| panic!("refused: {e}"));
    Counts {
        elements: report.work.elements,
        items: report.work.items,
        name_lookups: report.work.name_lookups,
        steps: written.work.steps_used(),
        bytes: written.bytes.len(),
    }
}

/// The yardstick: `shapes` rectangles and `connectors` connectors, each connector glued to the first rectangle.
fn yardstick(shapes: usize, connectors: usize) -> Drawing {
    let page: String = (0..shapes)
        .map(|k| rect_with(&format!(r#"xml:id="r{k}" draw:id="r{k}""#), ""))
        .chain((0..connectors).map(|k| connector_to(&format!("r{}", k % shapes.max(1)))))
        .collect();
    on_gr1(&page)
}

fn points(n: usize) -> String {
    (0..n).map(|k| format!(r#"<draw:glue-point draw:id="{}" svg:x="{}cm" svg:y="1cm"/>"#, k + 4, k % 7)).collect()
}

/// One shape with many glue points writes each once: four times the points cost four times the steps of the save and the items of the read, and the points are all there.
#[test]
fn the_glue_points_of_a_shape_are_written_once_each() {
    let none = counts(&on_gr1(&rect_with("", "")));
    let (small, large) = (counts(&on_gr1(&rect_with("", &points(1_000)))), counts(&on_gr1(&rect_with("", &points(4_000)))));
    for (n, c) in [(1_000u64, &small), (4_000, &large)] {
        assert!(c.steps - none.steps <= 40 * n, "{n}: the steps of the save: {c:?}");
        assert!(c.items - none.items <= 80 * n, "{n}: the items of the read: {c:?}");
    }
    assert!(large.steps - none.steps <= 4 * (small.steps - none.steps) + 200, "four times the points, four times the steps: {small:?} {large:?}");
    assert!(large.bytes <= 4 * small.bytes + 4_000, "the file grows in proportion: {small:?} {large:?}");
    // Every point is in the file written.
    let source = source_of(&on_gr1(&rect_with("", &points(4_000))));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let content = crate::visit_styles::part(&written.bytes, "content.xml");
    assert_eq!(content.matches("<draw:glue-point ").count(), 4_000);
}

/// Many connectors naming one target id cost a lookup each, in a map: four times the connectors cost four times the lookups of the read and the steps of the save, never their product with
/// the shapes.
#[test]
fn many_connectors_naming_one_target_cost_one_lookup_each() {
    let counts_of = |connectors: usize| counts(&yardstick(2_000, connectors));
    let (small, large) = (counts_of(2_000), counts_of(8_000));
    let base = counts(&yardstick(2_000, 0));
    assert!(large.name_lookups - base.name_lookups <= 4 * (small.name_lookups - base.name_lookups) + 100, "{small:?} {large:?}");
    assert!(
        large.steps - base.steps <= 4 * (small.steps - base.steps) + 2_000,
        "four times the connectors, four times the steps: {base:?} {small:?} {large:?}"
    );
    assert!(large.elements - base.elements <= 4 * (small.elements - base.elements) + 100, "{small:?} {large:?}");
}

/// Two shapes that claim one id, and an id of a few hundred kibibytes, are written as they were read, the id once on each attribute; and the ids that are more than a shape's kept markup
/// may be are refused in words.
#[test]
fn two_shapes_that_claim_one_id_and_a_long_id_are_written_as_read_and_one_past_the_bound_is_refused_in_words() {
    let long = "i".repeat(300 << 10);
    let page = format!(
        "{}{}{}",
        rect_with(&format!(r#"xml:id="{long}" draw:id="{long}""#), ""),
        rect_with(&format!(r#"xml:id="{long}" draw:id="{long}""#), ""),
        connector_to(&long)
    );
    let source = source_of(&on_gr1(&page));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let content = crate::visit_styles::part(&written.bytes, "content.xml");
    assert_eq!(
        content.matches(long.as_str()).count(),
        2 + 2 + 1,
        "both attributes of both shapes and the connector's end: the id held twice is written twice"
    );
    // An id of 600 KiB on both attributes is more than a shape keeps: refused in words, and nothing is written.
    let huge = "j".repeat(600 << 10);
    let source = source_of(&on_gr1(&rect_with(&format!(r#"xml:id="{huge}" draw:id="{huge}""#), "")));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    match write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("more than 1 MiB"), "{words}"),
        other => panic!("expected a refusal in words, got {:?}", other.map(|w| w.bytes.len())),
    }
}

/// The nested groups of D2: `depth` groups, each inside the one before, each with an automatic style of its own and glue points of its own, and no name, round a rectangle.
fn nested(depth: usize) -> Drawing {
    let styles: String = (0..depth)
        .map(|k| format!(r##"<style:style style:name="g{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#{:06x}"/></style:style>"##, 0x10_0000 + k))
        .collect();
    let mut page = rect_with("", "");
    for k in (0..depth).rev() {
        page = format!(r#"<draw:g draw:style-name="g{k}"><draw:glue-point draw:id="{}" svg:x="1cm" svg:y="1cm"/>{page}</draw:g>"#, k + 4);
    }
    Drawing::a4().auto(&styles).page(&page)
}

/// Groups nested deep, each with its own style and glue points, cost a constant each: twice the depth costs twice the steps and the items, never the square.
#[test]
fn unnamed_groups_nested_deep_with_styles_and_glue_points_cost_a_constant_each() {
    let (shallow, deep) = (counts(&nested(40)), counts(&nested(160)));
    assert!(deep.steps <= 4 * shallow.steps + 400, "four times the depth, four times the steps: {shallow:?} {deep:?}");
    assert!(deep.items <= 4 * shallow.items + 400, "{shallow:?} {deep:?}");
    assert!(deep.elements <= 4 * shallow.elements + 100, "{shallow:?} {deep:?}");
    // And it all comes back: each group with its style and its point.
    let source = source_of(&nested(160));
    let (doc, _) = read(&source, &Limits::default()).unwrap();
    let written = write_with(&through_codec(&doc), None, &WriteOptions { content_unchanged: true, ..WriteOptions::default() }).unwrap();
    let content = crate::visit_styles::part(&written.bytes, "content.xml");
    assert_eq!(content.matches("<draw:g ").count(), 160);
    assert_eq!(content.matches("<draw:glue-point ").count(), 160);
}

/// The reader's cap of depth, in words: groups nested past it are refused when they are read, and nothing is made.
#[test]
fn groups_nested_past_the_depth_the_reader_holds_are_refused_in_words_when_read() {
    match read(&source_of(&nested(192)), &Limits::default()) {
        Err(OdgError::TooComplex { what, .. }) => assert!(what.contains("nested"), "{what}"),
        other => panic!("expected the refusal, got {:?}", other.map(|(_, r)| r.work.elements)),
    }
}

/// The child's scenarios, one per call.
#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    match scenario.as_str() {
        "ids_past_the_bound" => {
            // Fifty thousand shapes each holding ids of 40 KiB are 4 GB of ids: the save is refused in words once it is past what it may copy, and never aborts.
            let id = "k".repeat(40 << 10);
            let one = rect_with(&format!(r#"xml:id="{id}" draw:id="{id}""#), "");
            let (doc, _) = read(&source_of(&on_gr1(&one)), &Limits::default()).unwrap();
            let mut doc = through_codec(&doc);
            let page = std::sync::Arc::make_mut(doc.pages.get_mut(0).unwrap());
            let shape = page.shapes.get(0).unwrap().clone();
            for _ in 0..50_000 {
                page.shapes.push(shape.clone());
            }
            match write_with(&doc, None, &WriteOptions::default()) {
                Err(_) => {}
                Ok(written) => assert!(written.bytes.len() > 1 << 20),
            }
        }
        other => panic!("no scenario {other}"),
    }
}

fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact visit_shapes_cost::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

/// A refusal is not an abort: fifty thousand copies of a shape with ids of 40 KiB are refused in words, or written, under 3 GB of address space, and the process ends well.
#[test]
fn fifty_thousand_shapes_with_long_ids_are_refused_in_words_and_do_not_abort() {
    let (ended_well, stdout) = run_child("ids_past_the_bound", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
}

/// D1, the real-time check (run by hand): the yardstick (20,000 shapes and 10,000 connectors), and 40,000 connectors naming one target (the plan says a million: a connector costs the reader a few hundred
/// of its twenty million items, so a million are refused by it in words before the writer is asked), each saved back to back; the second takes no more than four times the first (it holds
/// four times the connectors), and twice the connectors take no more than two and a half times as long.
#[test]
#[ignore = "a timing: run by hand with --nocapture"]
fn many_connectors_naming_one_target_take_no_more_than_a_linear_time_of_the_yardstick() {
    let time = |drawing: &Drawing| {
        let start = Instant::now();
        let c = counts(drawing);
        (start.elapsed(), c)
    };
    let (base, c0) = time(&yardstick(20_000, 10_000));
    let (half, c1) = time(&yardstick(1, 20_000));
    let (full, c2) = time(&yardstick(1, 40_000));
    eprintln!("yardstick {base:?} {c0:?}\n20,000 {half:?} {c1:?}\n40,000 {full:?} {c2:?}");
    assert!(full <= base * 4, "{full:?} against {base:?}");
    assert!(full.as_secs_f64() <= 2.5 * half.as_secs_f64(), "{full:?} against {half:?}");
}
