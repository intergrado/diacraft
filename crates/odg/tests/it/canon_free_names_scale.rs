//! What the content check costs when names are free, on drawings of 100,000 automatic styles. Every bound is a count of the work the check does
//! (`Work::canon_steps` and `Work::canon_bytes`), set a little above what was measured, and each is held to **a plain yardstick**: the check on the same two documents with the
//! rule of free names left out (`Rules { free_names: &[], ..CANON_RULES }`, which is the check as it was before the ruling), so that what is measured is what the rule adds. The timed
//! cases (`#[ignore]`, run with `cargo test -p diacraft-odg --test it canon_free_names_scale -- --ignored --test-threads=1`) hold each to plain content of the same size in time. The crafted
//! inputs:
//! - every style different and matched (the same 100,000 styles under other names, in another order);
//! - every style the same content (100,000 names for one style, on each side);
//! - every style different and none matched (nothing is renamed: the names are compared as they were before the ruling).

use std::time::{Duration, Instant};

use diacraft_container::Work;
use diacraft_container::canon::{Entry, Rules, compare_entries};
use diacraft_odg::{CANON_RULES, Limits, compare_content, compare_content_counted};

use crate::canon::{at, graphic, shape_with};
use crate::drawing::Drawing;

const N: usize = 100_000;

/// The multiplier that scrambles `0..n` into an order with no long runs (it has no factor in common with `N`).
const SCRAMBLE: usize = 65_537;

fn colour(i: usize) -> String {
    format!("#{i:06x}")
}

/// `n` graphic styles named `{prefix}{i}` whose content is `colour(i + shift)`, in the order of `i` or scrambled.
fn styles(n: usize, prefix: &str, shift: usize, scrambled: bool) -> String {
    (0..n)
        .map(|k| {
            let i = if scrambled { k * SCRAMBLE % n } else { k };
            graphic(&format!("{prefix}{i}"), &colour(i + shift))
        })
        .collect()
}

/// `n` graphic styles named `{prefix}{i}`, all of one content.
fn alike(n: usize, prefix: &str) -> String {
    (0..n).map(|i| graphic(&format!("{prefix}{i}"), "#336699")).collect()
}

fn drawing(auto: &str) -> Drawing {
    let mut d = Drawing::a4().page(&shape_with("s", &at("1cm"), "x"));
    d.auto = auto.to_owned();
    d
}

fn counted(a: &[u8], b: &[u8]) -> (u64, Work) {
    let mut work = Work::default();
    let diff = compare_content_counted(a, b, &Limits::default(), &mut work).expect("compares");
    (diff.count, work)
}

/// The work of the check on the `content.xml` of two drawings (where their styles are), with `rules`.
fn work_of(rules: &Rules, a: &Drawing, b: &Drawing) -> (u64, Work) {
    let (ca, cb) = (a.content_xml(), b.content_xml());
    let mut work = Work::default();
    let diff = compare_entries(
        &[Entry { name: "content.xml", xml: true, bytes: ca.as_bytes() }],
        &[Entry { name: "content.xml", xml: true, bytes: cb.as_bytes() }],
        false,
        rules,
        &Limits::default().container.xml,
        &mut work,
    )
    .expect("compares");
    (diff.count, work)
}

/// The check as it was before the ruling: every name as written.
fn as_before() -> Rules {
    Rules { free_names: &[], ..CANON_RULES }
}

/// Measured on each case (see the report: 1.58 to 2.25 times the steps and 1.0 to 2.79 times the bytes; added the walk of the references between the styles and the copies of a
/// class), the bounds a little above: what the rule adds to the steps and to the bytes the check counts (the build of the container's children is counted once and not again: what is
/// added is the weighing of the container, the content of each style written out once, its lookup, and a step for each style and each reference of the walk).
const STEPS_OVER_BEFORE: f64 = 2.5;
const BYTES_OVER_BEFORE: f64 = 3.1;

#[track_caller]
fn adds_little(what: &str, a: &Drawing, b: &Drawing) -> (u64, Work) {
    let (now_count, now) = work_of(&CANON_RULES, a, b);
    let (_, before) = work_of(&as_before(), a, b);
    let (steps, bytes) = (now.canon_steps as f64 / before.canon_steps as f64, now.canon_bytes as f64 / before.canon_bytes as f64);
    println!(
        "MEASURED {what}: steps {steps:.3} bytes {bytes:.3} times the check as it was ({} steps, {} bytes)",
        before.canon_steps, before.canon_bytes
    );
    assert!(steps <= STEPS_OVER_BEFORE, "{what}: {steps} times the steps");
    assert!(bytes <= BYTES_OVER_BEFORE, "{what}: {bytes} times the bytes");
    (now_count, now)
}

// ----------------------------------------------------------------------------------------------------------------------------- counted

#[test]
fn a_hundred_thousand_automatic_styles_under_other_names_are_the_same_and_the_rule_adds_a_pass() {
    let (first, second) = (drawing(&styles(N, "gr", 0, false)), drawing(&styles(N, "x", 0, true)));
    let (count, _) = counted(&first.bytes(), &second.bytes());
    assert_eq!(count, 0, "the same styles under other names, in another order");
    assert_eq!(adds_little("100,000 styles renamed", &first, &second).0, 0);
}

#[test]
fn a_hundred_thousand_names_for_one_style_are_the_same_and_the_rule_adds_a_pass() {
    let (first, second) = (drawing(&alike(N, "gr")), drawing(&alike(N, "other")));
    let (count, work) = counted(&first.bytes(), &second.bytes());
    assert_eq!(count, 0, "a hundred thousand styles of one content, under other names");
    assert_eq!(adds_little("100,000 styles of one content", &first, &second).0, 0);
    // One more on one side, of another content (copies of a style count as one): the one difference, and it costs the same.
    let more = drawing(&format!("{}{}", alike(N, "other"), graphic("one_more", "#336698")));
    let (count, again) = counted(&first.bytes(), &more.bytes());
    assert_eq!(count, 1);
    assert!(again.canon_steps as f64 <= work.canon_steps as f64 * 1.05 + 1_000.0, "{} against {}", again.canon_steps, work.canon_steps);
}

#[test]
fn a_hundred_thousand_different_styles_with_no_match_are_compared_as_they_were_and_the_rule_adds_a_pass() {
    // Nothing matches: no name is read as its content, and every style is a style on one side only (or the one of its name, when the names agree).
    let (first, second) = (drawing(&styles(N, "gr", 0, false)), drawing(&styles(N, "gr", N, false)));
    assert_eq!(adds_little("100,000 styles with no match", &first, &second).0, N as u64, "each style of one name differs in its colour");
    // Other names as well: each is a style on one side only, two differences for each.
    let third = drawing(&styles(N, "z", N, true));
    assert_eq!(adds_little("100,000 styles with no match and no name in common", &first, &third).0, 2 * N as u64);
}

/// `n` pairs of a text style that names a list style and a list style whose level names the next text style: a chain of `2 * n` definitions, one that names no definition at its end, or a
/// circle when `circle` (the last names the first).
fn chained(prefix: &str, n: usize, circle: bool) -> String {
    (0..n)
        .map(|i| {
            let next = if i + 1 < n { format!("{prefix}T{}", i + 1) } else if circle { format!("{prefix}T0") } else { "Common".to_owned() };
            format!(
                "<style:style style:name=\"{prefix}T{i}\" style:family=\"text\" style:list-style-name=\"{prefix}L{i}\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
                 <text:list-style style:name=\"{prefix}L{i}\"><text:list-level-style-bullet text:level=\"1\" text:style-name=\"{next}\" text:bullet-char=\"o\"/></text:list-style>"
            )
        })
        .collect()
}

#[test]
fn a_chain_of_references_ten_thousand_deep_and_a_circle_of_them_are_compared_as_written_in_a_pass() {
    // A chain longer than the check follows and a circle are compared as written: the names differ, so the files do, and the work is a few passes, as for any other
    // 10,000 styles (the check as it was is the yardstick).
    let n = 5_000;
    let (a, b) = (drawing(&chained("a", n, false)), drawing(&chained("b", n, false)));
    let (count, _) = adds_little("a chain of 10,000 styles", &a, &b);
    assert!(count > 0, "a chain too long is compared as written");
    let (a, b) = (drawing(&chained("a", n, true)), drawing(&chained("b", n, true)));
    let (count, _) = adds_little("a circle of 10,000 styles", &a, &b);
    assert!(count > 0, "a circle is compared as written");
    // The same circle written as it was is the same.
    let (a, b) = (drawing(&chained("a", n, true)), drawing(&chained("a", n, true)));
    assert_eq!(counted(&a.bytes(), &b.bytes()).0, 0);
}

#[test]
fn the_work_grows_with_the_styles_and_no_faster() {
    let cost = |n: usize| {
        let (a, b) = (drawing(&styles(n, "gr", 0, false)).bytes(), drawing(&styles(n, "x", 0, true)).bytes());
        let (count, work) = counted(&a, &b);
        assert_eq!(count, 0);
        work
    };
    let (small, large) = (cost(N / 10), cost(N));
    // A tenth as many cost a tenth of the steps (the keyed sort is an n log n: it may cost a little more than a tenth, never ten times that).
    assert!(
        small.canon_steps as f64 * 10.0 <= large.canon_steps as f64 * 1.05 + 5_000.0,
        "{} for a tenth, {} for all",
        small.canon_steps,
        large.canon_steps
    );
    assert!(
        small.canon_bytes as f64 * 10.0 <= large.canon_bytes as f64 * 1.3 + 50_000.0,
        "{} for a tenth, {} for all",
        small.canon_bytes,
        large.canon_bytes
    );
}

// ----------------------------------------------------------------------------------------------------------------------------- timed

/// The yardstick in time: plain content of the same size, `bytes` of shapes, and a copy of it with one value changed (the one difference a round trip finds).
fn plain(bytes: usize) -> (Vec<u8>, Vec<u8>) {
    let one = shape_with("s", &at("1cm"), "x").len();
    let shapes = bytes / one;
    let build = |salt: bool| {
        let body: String = (0..shapes)
            .map(|i| {
                let x = if salt && i == shapes / 2 { "9cm".to_owned() } else { format!("{}cm", i % 7) };
                shape_with(&format!("s{i}"), &at(&x), "x")
            })
            .collect();
        Drawing::a4().page(&body).bytes()
    };
    (build(false), build(true))
}

fn timed(a: &[u8], b: &[u8]) -> Duration {
    let start = Instant::now();
    let diff = compare_content(a, b, &Limits::default()).expect("compares");
    std::hint::black_box(diff.count);
    start.elapsed()
}

#[track_caller]
fn five_times(case: &str, time: Duration, yard: Duration) {
    eprintln!("{case}: {time:?} against the yardstick's {yard:?} ({:.2}x)", time.as_secs_f64() / yard.as_secs_f64());
    assert!(time <= yard * 5, "{case}: {time:?} is more than five times the yardstick's {yard:?}");
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn the_crafted_drawings_take_no_more_than_five_times_plain_content_of_the_same_size() {
    let first = styles(N, "gr", 0, false);
    let (a, b) = plain(first.len());
    let yard = timed(&a, &b);
    five_times("100,000 styles renamed", timed(&drawing(&first).bytes(), &drawing(&styles(N, "x", 0, true)).bytes()), yard);
    five_times("100,000 names for one style", timed(&drawing(&alike(N, "gr")).bytes(), &drawing(&alike(N, "o")).bytes()), yard);
    five_times("100,000 styles with no match", timed(&drawing(&first).bytes(), &drawing(&styles(N, "gr", N, false)).bytes()), yard);
}
