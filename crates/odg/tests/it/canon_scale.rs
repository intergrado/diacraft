//! What the content check costs on crafted input (done-criteria): every bound is a count of the work the check does (`Work::canon_steps` and
//! `Work::canon_bytes`), set a little above what was measured, and the timed cases (`#[ignore]`, run with
//! `cargo test -p diacraft-odg --test it canon_scale -- --ignored --test-threads=1`) hold each to a yardstick of plain content of the same size.
//!
//! The crafted inputs of the task: 65,536 styles reordered, and a `text:p` of 1,000,000 `text:s` elements; and the flat path's own, a flat file of as
//! many styles against its package.

use std::time::{Duration, Instant};

use diacraft_container::Work;
use diacraft_odg::{Limits, compare_content, compare_content_counted};

use crate::canon::{at, shape_with};
use crate::drawing::Drawing;
use crate::flat::Flat;

/// The multiplier that scrambles `0..n` into an order with no long runs: it is odd, so a permutation of 65,536 and of every power of two below.
const SCRAMBLE: usize = 40_503;

fn style(k: usize) -> String {
    format!(
        r##"<style:style style:name="s{k}" style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#000000"/></style:style>"##
    )
}

/// The same `n` styles (`n` a power of two) in order, or in a quasi-random order.
fn styles(n: usize, scrambled: bool) -> String {
    assert!(n.is_power_of_two());
    (0..n).map(|i| style(if scrambled { i * SCRAMBLE % n } else { i })).collect()
}

fn with_styles(common: &str) -> Drawing {
    let mut d = Drawing::a4();
    d.common = common.to_owned();
    // A face, so that no container is empty: a flat file of the same pieces holds none that is.
    d.fonts = r#"<style:font-face style:name="Mono" svg:font-family="Mono"/>"#.to_owned();
    d
}

fn counted(a: &[u8], b: &[u8]) -> (bool, Work) {
    let mut work = Work::default();
    let diff = compare_content_counted(a, b, &Limits::default(), &mut work).expect("compares");
    (diff.same(), work)
}

fn n_log_n(n: usize) -> f64 {
    n as f64 * (n as f64).log2()
}

/// The package of `d` whose `styles.xml` ends in a comment after its root: other bytes, the same content, so that a comparison never takes the shortcut of
/// equal bytes and parses both sides whole.
fn noted(d: &Drawing) -> Vec<u8> {
    let mut files = d.files();
    for (name, bytes) in &mut files {
        if name == "styles.xml" || name == "content.xml" {
            bytes.extend_from_slice(b"\n<!-- end -->\n");
        }
    }
    crate::build::package(crate::build::DRAWING, files)
}

// ----------------------------------------------------------------------------------------------------------------------------- counted

/// Measured on 65,536 styles in order against the same in a quasi-random order (see the report), the bound set a little above.
const STEPS_PER_N_LOG_N: f64 = 3.4;
const BYTES_PER_N_LOG_N: f64 = 24.0;

#[test]
fn styles_reordered_cost_n_log_n_at_the_most_and_65536_of_them_are_the_same() {
    let n = 65_536;
    let (a, b) = (with_styles(&styles(n, false)).bytes(), with_styles(&styles(n, true)).bytes());
    let (same, work) = counted(&a, &b);
    assert!(same);
    let (steps, bytes) = (work.canon_steps as f64 / n_log_n(n), work.canon_bytes as f64 / n_log_n(n));
    println!("MEASURED styles steps {steps:.3} bytes {bytes:.3} per n log2 n");
    assert!(steps <= STEPS_PER_N_LOG_N, "{steps} steps for each n log2 n");
    assert!(bytes <= BYTES_PER_N_LOG_N, "{bytes} bytes for each n log2 n");
    // The cost is an n log n and no worse: a quarter as many cost a bit less than a quarter of the work.
    let small = 16_384;
    let (small_same, small_work) = counted(&with_styles(&styles(small, false)).bytes(), &with_styles(&styles(small, true)).bytes());
    assert!(small_same);
    assert!(
        small_work.canon_steps as f64 * 4.0 <= work.canon_steps as f64 * 1.05,
        "{} for {small}, {} for {n}",
        small_work.canon_steps,
        work.canon_steps
    );
}

fn text_of(text: &str) -> Drawing {
    Drawing::a4().page(&shape_with("s", &at("1cm"), text))
}

/// Measured on a million `text:s` against the same with a comment (see the report), the bound set a little above.
const STEPS_PER_SPACE: f64 = 2.2;
const BYTES_PER_SPACE: f64 = 0.01;

#[test]
fn a_paragraph_of_a_million_text_s_costs_a_pass() {
    let n = 1_000_000usize;
    let d = text_of(&"<text:s/>".repeat(n));
    let (same, work) = counted(&d.bytes(), &noted(&d));
    assert!(same, "a comment after the root is no content");
    let (steps, bytes) = (work.canon_steps as f64 / n as f64, work.canon_bytes as f64 / n as f64);
    println!("MEASURED text:s steps {steps:.3} bytes {bytes:.3} per element");
    assert!(steps <= STEPS_PER_SPACE, "{steps} steps for each `text:s`");
    assert!(bytes <= BYTES_PER_SPACE, "{bytes} bytes for each");
    // Linear: a quarter of them cost a quarter, to within the parts' fixed cost.
    let q = text_of(&"<text:s/>".repeat(n / 4));
    let (_, quarter) = counted(&q.bytes(), &noted(&q));
    assert!(
        quarter.canon_steps as f64 * 4.0 <= work.canon_steps as f64 * 1.05 + 4_000.0,
        "{} for a quarter, {} for all",
        quarter.canon_steps,
        work.canon_steps
    );
}

#[test]
fn a_million_text_s_are_one_text_s_of_a_million_and_cost_a_pass() {
    let n = 1_000_000usize;
    let (spaces, one) = (text_of(&"<text:s/>".repeat(n)).bytes(), text_of(&format!("<text:s text:c=\"{n}\"/>")).bytes());
    let (same, work) = counted(&spaces, &one);
    assert!(same, "a million explicit spaces are one `text:s` of a million");
    assert!(work.canon_steps as f64 / n as f64 <= STEPS_PER_SPACE, "{} steps for each", work.canon_steps as f64 / n as f64);
}

// ----------------------------------------------------------------------------------------------------------------------------- the flat path

/// Measured on a flat file of 20,000 styles against its package (see the report), the bounds set a little above.
const FLAT_STEPS_PER_ELEMENT: f64 = 33.0;
const FLAT_OVER_PACKAGE: f64 = 1.5;

#[test]
fn a_flat_file_of_many_styles_costs_a_few_passes_against_its_package() {
    let n = 16_384;
    let d = with_styles(&styles(n, true));
    let package = d.bytes();
    let (same, flat) = counted(Flat::of(&with_styles(&styles(n, false))).bytes().as_slice(), &package);
    assert!(same);
    let steps = flat.canon_steps as f64 / (2 * n) as f64;
    println!("MEASURED flat steps {steps:.3} per element");
    assert!(steps <= FLAT_STEPS_PER_ELEMENT, "{steps} steps for each element");
    let (_, plain) = counted(&package, &with_styles(&styles(n, false)).bytes());
    println!("MEASURED flat {} package {}", flat.canon_steps, plain.canon_steps);
    assert!(flat.canon_steps as f64 <= plain.canon_steps as f64 * FLAT_OVER_PACKAGE, "{} against {}", flat.canon_steps, plain.canon_steps);
}

// ----------------------------------------------------------------------------------------------------------------------------- timed

/// The time of one comparison, the better of three.
fn time(a: &[u8], b: &[u8]) -> Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let diff = compare_content(a, b, &Limits::default()).expect("compares");
            assert!(diff.same());
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

fn within(case: Duration, yardstick: Duration, what: &str) {
    println!("{what}: case {case:?}, yardstick {yardstick:?}, ratio {:.2}", case.as_secs_f64() / yardstick.as_secs_f64());
    assert!(case.as_secs_f64() <= yardstick.as_secs_f64() * 3.0, "{what}: {case:?} against {yardstick:?}");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_styles_reordered_against_plain_siblings_of_the_same_size() {
    let n = 65_536;
    let case = time(&with_styles(&styles(n, false)).bytes(), &with_styles(&styles(n, true)).bytes());
    // The yardstick: as many elements of the same size that no rule keys (`style:default-style` is not a keyed child), in one order, against the same with
    // a comment after the root, so that the parse and the canonical form are the whole work.
    let one = |k: usize| {
        format!(
            r##"<style:default-style style:family="f{k}"><style:graphic-properties draw:fill="solid" draw:fill-color="#000000"/></style:default-style>"##
        )
    };
    let plain = with_styles(&(0..n).map(one).collect::<String>());
    within(case, time(&plain.bytes(), &noted(&plain)), "65,536 styles reordered");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_a_million_text_s_against_plain_content_of_the_same_size() {
    let n = 1_000_000usize;
    let spaces = text_of(&"<text:s/>".repeat(n));
    let case = time(&spaces.bytes(), &noted(&spaces));
    // The yardstick: a million elements of the same size that are no spaces, against the same with a comment.
    let plain = text_of(&"<text:a/>".repeat(n));
    within(case, time(&plain.bytes(), &noted(&plain)), "1,000,000 text:s");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_many_declarations_and_children_against_plain_children_of_the_same_size() {
    let n = 50_000;
    let (flat, pkg) = crate::canon_hostile::declaring(n, n, "urn:p", n);
    let case = time(&flat, &pkg);
    // The yardstick: as many children (the declarations are as many attributes: the same number of items again as plain children), no prefix bound.
    let (flat, pkg) = crate::canon_hostile::declaring(0, 2 * n, "", 1);
    within(case, time(&flat, &pkg), "50,000 declarations and 50,000 children");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_a_container_of_many_attributes_in_both_parts_against_plain_children_of_the_same_size() {
    let n = 50_000;
    let (flat, pkg) = crate::canon_hostile::crowded(n);
    let case = time(&flat, &pkg);
    let (flat, pkg) = crate::canon_hostile::declaring(0, 2 * n, "", 1);
    within(case, time(&flat, &pkg), "two containers of 50,000 attributes");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_a_flat_file_against_its_package_and_against_plain_content() {
    let n = 16_384;
    let case = time(&Flat::of(&with_styles(&styles(n, false))).bytes(), &with_styles(&styles(n, true)).bytes());
    // The yardstick: the same number of styles that no rule keys, flat against its package, which does the same passes without a sort.
    let one = |k: usize| {
        format!(
            r##"<style:default-style style:family="f{k}"><style:graphic-properties draw:fill="solid" draw:fill-color="#000000"/></style:default-style>"##
        )
    };
    let plain = with_styles(&(0..n).map(one).collect::<String>());
    within(case, time(&Flat::of(&plain).bytes(), &plain.bytes()), "a flat file of 16,384 styles");
}

#[test]
#[ignore = "timed against a yardstick: run alone"]
fn timed_automatic_styles_in_both_parts_against_plain_content_of_the_same_size() {
    // The shared container: 16,384 automatic styles in each part, joined into the flat file's one container and each given its name.
    let n = 16_384;
    let mut d = with_styles(&style(0));
    d.styles_auto = format!("{}{}", d.styles_auto, (0..n).map(|k| style(k).replacen("style:name=\"s", "style:name=\"a", 1)).collect::<String>());
    d.auto = (0..n).map(|k| style(k).replacen("style:name=\"s", "style:name=\"c", 1)).collect();
    let case = time(&Flat::of(&d).bytes(), &d.bytes());
    // The yardstick: twice as many styles that no rule keys, in the common styles, which no join and no name is made for.
    let one = |k: usize| {
        format!(
            r##"<style:default-style style:family="f{k}"><style:graphic-properties draw:fill="solid" draw:fill-color="#000000"/></style:default-style>"##
        )
    };
    let plain = with_styles(&(0..2 * n).map(one).collect::<String>());
    within(case, time(&Flat::of(&plain).bytes(), &plain.bytes()), "16,384 automatic styles in each of two parts");
}
