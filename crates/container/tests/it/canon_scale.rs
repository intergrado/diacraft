//! The content check at the sizes the limits allow, timed back to back with its yardstick (done-criterion D1). All `#[ignore]`: they take
//! seconds and gigabytes, and no default-run test asserts a time (`canon.rs` holds the same inputs at a smaller size, counted).
//!
//! **The yardstick** is the check on plain content of the same number of nodes: the flat `<e>` elements of a real part (a shape of sixteen
//! cells, say), compared with a copy of itself that differs in one value, which is what a round trip finds. A crafted input fails when it
//! takes more than five times as long as that: the check is linear in the nodes and `n log n` in the siblings of a key set, and the factor
//! is the log, the sort's own work and the cache (the plan says "slower than the yardstick"; the sort of 100,000 keys in a random order
//! measures 2.3 times it, and of a million 3.2 to 3.5 on a loaded machine, and the rest 0.9 to 1.3). The yardstick is run in the same process
//! and the same minute, so that the load of the machine is in both. A quadratic blow-up is a hundred times, and a million keys would be ten
//! thousand, not five.
//!
//! Run with `cargo test -p diacraft-container --test it canon_scale -- --ignored --test-threads=1`, one at a time: the largest holds about
//! 1.5 GB (the peaks are printed).

use std::time::{Duration, Instant};

use diacraft_container::canon::{ContentDiff, Entry, KeyFold, Keyed, Name, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};

const KEY: &[Name] = &[Name { ns: "", local: "n" }];
const RULES: Rules = Rules {
    keyed: &[Keyed { child: Name { ns: "", local: "c" }, key: KEY, fold: KeyFold::Exact }],
    numbers: &[],
    lengths: &[],
    qnames: &[],
    paragraphs: &[],
    exact_text: &[],
    absent_when_empty: &[],
    absent_when_empty_under: &[],
    unordered: &[],
    parts_absent_when_empty: &[],
    ignored_attributes: &[],
    ignored_children: &[],
    ignored_when_extra: &[],
    twinned: &[],
    free_names: &[],
    placed: &[],
    aliases: &[],
    identified_by: &[],
    doctype_parts: &[],
};

/// The peak resident memory of this process so far, in bytes.
fn peak_rss() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}

fn timed(a: &str, b: &str, limits: &XmlLimits) -> (Duration, ContentDiff, Work) {
    timed_with(&RULES, a, b, limits)
}

fn timed_with(rules: &Rules, a: &str, b: &str, limits: &XmlLimits) -> (Duration, ContentDiff, Work) {
    let mut work = Work::default();
    let start = Instant::now();
    let d = compare_entries(
        &[Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }],
        &[Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }],
        true,
        rules,
        limits,
        &mut work,
    )
    .unwrap();
    (start.elapsed(), d, work)
}

/// [`RULES`], and the attribute `id` says which thing an element is.
const IDENTIFIED: Rules = Rules { identified_by: &[Name { ns: "", local: "id" }], ..RULES };

fn deep_limits() -> XmlLimits {
    XmlLimits { max_depth: 512, ..XmlLimits::default() }
}

/// The yardstick: `nodes` nodes of plain shapes (an element, then sixteen cells of an element and two attributes), a copy with one value changed.
fn yardstick(nodes: usize) -> (String, String) {
    let shapes = nodes / (1 + 16 * 3);
    let build = |salt: bool| {
        let mut s = String::with_capacity(nodes * 12);
        s.push_str("<Page>");
        for shape in 0..shapes {
            s.push_str(&format!("<Shape ID=\"{shape}\">"));
            for cell in 0..16 {
                let v = if salt && shape == shapes / 2 && cell == 3 { "salt" } else { "1.5" };
                s.push_str(&format!("<Cell{cell} N=\"x\" V=\"{v}\"/>"));
            }
            s.push_str("</Shape>");
        }
        s.push_str("</Page>");
        s
    };
    (build(false), build(true))
}

fn yardstick_time(nodes: usize) -> Duration {
    let (a, b) = yardstick(nodes);
    let (time, d, _) = timed(&a, &b, &deep_limits());
    assert_eq!(d.count, 1);
    time
}

fn within(case: &str, time: Duration, yard: Duration) {
    eprintln!("{case}: {time:?} against the yardstick's {yard:?} ({:.2}x)", time.as_secs_f64() / yard.as_secs_f64());
    assert!(time <= yard * 5, "{case}: {time:?} is more than five times the yardstick's {yard:?}");
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_hundred_thousand_keyed_siblings_with_one_moved() {
    let n = 100_003u64;
    let cells = |moved: bool, multiplier: u64| {
        let mut s = String::from("<r>");
        for i in 0..n {
            let at = if moved && i == 0 { n } else { (i * multiplier) % n };
            s.push_str(&format!("<c n=\"K{at:08}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    // The orders are quasi-random (a multiplier near the golden ratio's share of n, n prime): a few long ascending runs would sort in a pass.
    let (a, b) = (n * 618 / 1000, n * 414 / 1000);
    let (time, d, work) = timed(&cells(false, a), &cells(true, b), &deep_limits());
    assert_eq!(d.count, 2);
    eprintln!("steps {} ({:.2} per n log2 n)", work.canon_steps, work.canon_steps as f64 / (n as f64 * (n as f64).log2()));
    within("100,000 keyed siblings", time, yardstick_time(2 * n as usize));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_hundred_thousand_siblings_that_share_one_key() {
    let n = 100_000usize;
    let cells = |salt: bool| {
        let mut s = String::from("<r>");
        for i in 0..n {
            s.push_str(&format!("<c n=\"same\" v=\"{}\"/>", if salt && i == n / 2 { "x" } else { "y" }));
        }
        s.push_str("</r>");
        s
    };
    let (time, d, _) = timed(&cells(false), &cells(true), &deep_limits());
    assert_eq!(d.count, 1);
    within("100,000 siblings of one key", time, yardstick_time(3 * n));
}

#[test]
#[ignore = "about 1.3 GB and some seconds; run with --ignored, alone"]
fn a_part_at_the_item_budget() {
    // 666,666 elements of two attributes, and the root: 1,999,999 items, one under the budget of 2,000,000 (`MAX_COMPARE_ITEMS`).
    let n = 666_666usize;
    let build = |salt: bool| {
        let mut s = String::with_capacity(n * 22 + 16);
        s.push_str("<r>");
        for i in 0..n {
            s.push_str(if salt && i == n / 2 { "<e a=\"1\" b=\"salt\"/>" } else { "<e a=\"1\" b=\"2\"/>" });
        }
        s.push_str("</r>");
        s
    };
    let (a, b) = (build(false), build(true));
    let before = peak_rss();
    let (time, d, work) = timed(&a, &b, &XmlLimits::default());
    assert_eq!(d.count, 1);
    eprintln!(
        "the item budget: {time:?}, {} steps ({:.2} per element), {} bytes, peak resident {} MB (before {} MB, the inputs)",
        work.canon_steps,
        work.canon_steps as f64 / n as f64,
        work.canon_bytes,
        peak_rss() >> 20,
        before >> 20
    );
    assert!(work.canon_steps <= 15 * n as u64);
    within("the item budget", time, yardstick_time(1_950_000));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_part_past_the_item_budget_is_refused_in_a_fraction_of_the_time_it_would_take() {
    // 6 million items, which the parser allows and the budget does not: refused from the count, after reading the first 2 million.
    let n = 2_000_000usize;
    let part = format!("<r>{}</r>", "<e a=\"1\" b=\"2\"/>".repeat(n));
    let start = Instant::now();
    let err = compare_entries(
        &[Entry { name: "/p.xml", xml: true, bytes: part.as_bytes() }],
        &[Entry { name: "/p.xml", xml: true, bytes: part.replace("\"2\"", "\"3\"").as_bytes() }],
        true,
        &RULES,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap_err();
    let refused = start.elapsed();
    assert_eq!(err.class(), "xml_too_complex");
    eprintln!("6,000,001 items refused in {refused:?}, peak resident {} MB", peak_rss() >> 20);
}

#[test]
#[ignore = "about 1.4 GB and some seconds; run with --ignored, alone"]
fn distinct_keyed_siblings_at_the_item_budget() {
    // 999,999 keyed cells of an element and an attribute each, and the root: 1,999,999 items, in two quasi-random orders (n prime), one key
    // changed.
    let n = 999_983u64;
    let cells = |moved: bool, multiplier: u64| {
        let mut s = String::with_capacity(n as usize * 20 + 16);
        s.push_str("<r>");
        for i in 0..n {
            let at = if moved && i == 0 { n } else { (i * multiplier) % n };
            s.push_str(&format!("<c n=\"K{at:08}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    let (a, b) = (n * 618 / 1000, n * 414 / 1000);
    let (x, y) = (cells(false, a), cells(true, b));
    let before = peak_rss();
    let (time, d, work) = timed(&x, &y, &XmlLimits::default());
    assert_eq!(d.count, 2);
    eprintln!(
        "steps {} ({:.2} per n log2 n), peak resident {} MB (before {} MB)",
        work.canon_steps,
        work.canon_steps as f64 / (n as f64 * (n as f64).log2()),
        peak_rss() >> 20,
        before >> 20
    );
    within("distinct keyed siblings at the item budget", time, yardstick_time(1_950_000));
}

#[test]
#[ignore = "about 1.5 GB and some seconds; run with --ignored, alone"]
fn text_parts_at_the_byte_budget() {
    // Two paragraphs of text, one text node of 191 MiB on each side (382 MiB of the 384 the budget holds), differing in the last letter: the
    // tree holds each text, the paragraph builds each, and the parts are held by the caller.
    let words = "a ".repeat(95 << 20);
    let doc = |last: &str| {
        format!("<o:t xmlns:o=\"urn:o\" xmlns:text=\"{}\"><text:p>{words}{last}</text:p></o:t>", diacraft_container::canon::ODF_TEXT_NAMESPACE)
    };
    let (a, b) = (doc("x"), doc("y"));
    let rules = Rules { paragraphs: &[Name { ns: diacraft_container::canon::ODF_TEXT_NAMESPACE, local: "p" }], ..Rules::NONE };
    let before = peak_rss();
    let start = Instant::now();
    let d = compare_entries(
        &[Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }],
        &[Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() }],
        true,
        &rules,
        &XmlLimits::default(),
        &mut Work::default(),
    )
    .unwrap();
    assert_eq!(d.count, 1);
    eprintln!(
        "two paragraphs of {} MiB: {:?}, peak resident {} MB (before {} MB, the inputs)",
        a.len() >> 20,
        start.elapsed(),
        peak_rss() >> 20,
        before >> 20
    );
}

/// `chains` chains of `depth` nested elements side by side, each ending in an element with an attribute, the last-but-middle one's value changed
/// when `salt`.
fn chains(chains: usize, depth: usize, salt: bool) -> String {
    let mut s = String::from("<r>");
    for chain in 0..chains {
        s.push_str(&"<k>".repeat(depth));
        s.push_str(if salt && chain == chains / 2 { "<x v=\"salt\"/>" } else { "<x v=\"1\"/>" });
        s.push_str(&"</k>".repeat(depth));
    }
    s.push_str("</r>");
    s
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn nesting_at_the_depth_limit_many_times() {
    // 5,000 chains of 190 nested elements side by side: a million nodes at nearly the limit of 200. The yardstick is the same elements nested
    // ten times less deep (50,000 chains of 19), so that what is measured is the depth and nothing else: the cost of an element that holds one
    // (its lists, its box) is in both.
    let (time, d, work) = timed(&chains(5_000, 190, false), &chains(5_000, 190, true), &XmlLimits::default());
    assert_eq!(d.count, 1);
    eprintln!("steps {}", work.canon_steps);
    let (yard, d, yard_work) = timed(&chains(50_000, 19, false), &chains(50_000, 19, true), &XmlLimits::default());
    assert_eq!(d.count, 1);
    eprintln!("yardstick steps {}", yard_work.canon_steps);
    within("5,000 chains 190 deep", time, yard);
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn the_same_children_in_another_order_at_every_level() {
    // Depth 190, and at every level 400 siblings that stand in another order on the other side: an `Order` at each level, and each level
    // sorts its own 400 (a chain in which each item rests on the one before).
    let build = |reverse: bool| {
        let mut s = String::new();
        for _ in 0..190 {
            s.push_str("<k>");
            let ids: Vec<usize> = if reverse { (0..400).rev().collect() } else { (0..400).collect() };
            for id in ids {
                s.push_str(&format!("<s i=\"{id}\"/>"));
            }
        }
        s.push_str(&"</k>".repeat(190));
        s
    };
    let (time, d, work) = timed(&build(false), &build(true), &XmlLimits::default());
    assert!(d.count >= 190, "{}", d.count);
    eprintln!("steps {}", work.canon_steps);
    within("400 siblings reordered at each of 190 levels", time, yardstick_time(190 * 400 * 2));
}

/// `n` siblings of one name with an id each and a value, `changed` or not, with one more put in first when `put`.
fn pages(n: usize, changed: bool, put: bool) -> String {
    let mut s = String::from("<r>");
    if put {
        s.push_str("<page id=\"fresh\" v=\"0\"/>");
    }
    for i in 0..n {
        s.push_str(&format!("<page id=\"{i}\" v=\"{}\"/>", if changed { "b" } else { "a" }));
    }
    s.push_str("</r>");
    s
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_hundred_thousand_siblings_told_by_an_id_all_changed_with_one_put_in() {
    // Set side by side by their ids, each of the hundred thousand is compared with its own (a hundred thousand and one differences), in three sorts of the children and a
    // pass, and no more than five times the plain content of as many nodes.
    let n = 100_000usize;
    let (time, d, work) = timed_with(&IDENTIFIED, &pages(n, false, false), &pages(n, true, true), &deep_limits());
    assert_eq!(d.count, n as u64 + 1);
    assert_eq!(work.canon_by_place, 0);
    eprintln!("steps {} ({:.1} for each sibling)", work.canon_steps, work.canon_steps as f64 / n as f64);
    within("100,000 siblings told by an id, one put in", time, yardstick_time(2 * 3 * n));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_hundred_thousand_siblings_of_one_name_all_changed_with_one_put_in() {
    // Nothing tells them apart (one name, no id): nothing is matched by content, the name has a hundred thousand and one on one side, and they are set side by side by place.
    let n = 100_000usize;
    let (time, d, work) = timed_with(&RULES, &pages(n, false, false), &pages(n, true, true), &deep_limits());
    assert!(d.count >= n as u64, "{}", d.count);
    assert_eq!(work.canon_by_place, 0);
    within("100,000 siblings of one name, one put in", time, yardstick_time(2 * 3 * n));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_hundred_thousand_siblings_in_the_reverse_order_are_one_order() {
    let n = 100_000usize;
    let list = |reverse: bool| {
        let mut ids: Vec<usize> = (0..n).collect();
        if reverse {
            ids.reverse();
        }
        let mut s = String::from("<r>");
        for i in ids {
            s.push_str(&format!("<g id=\"{i}\"/>"));
        }
        s.push_str("</r>");
        s
    };
    let (time, d, _) = timed_with(&IDENTIFIED, &list(false), &list(true), &deep_limits());
    assert_eq!(d.count, 1);
    within("100,000 siblings in the reverse order", time, yardstick_time(2 * 2 * n));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn a_list_past_the_cap_is_set_side_by_side_by_place_in_the_time_of_the_old_reading() {
    // Past `MAX_ALIGNED` the list is set by place, as every list was: counted, and no slower than the plain content of as many nodes.
    let n = diacraft_container::canon::MAX_ALIGNED + 50_000;
    let (time, d, work) = timed_with(&IDENTIFIED, &pages(n, false, false), &pages(n, true, true), &deep_limits());
    assert!(d.count >= n as u64);
    assert!(work.canon_by_place >= 2 * n as u64, "{}", work.canon_by_place);
    within("a list past the cap", time, yardstick_time(2 * 3 * n));
}

#[test]
#[ignore = "seconds of CPU; run with --ignored, one at a time"]
fn fifty_thousand_parts_against_fifty_thousand() {
    let n = 50_000usize;
    let names: Vec<String> = (0..n).map(|i| format!("/{}/pages/page{:05}.xml", concat!("vi", "sio"), (i * 7919) % n)).collect();
    let part = |i: usize, spaced: bool| {
        if spaced { format!("<r a=\"{i}\" b=\"2\" > <c n=\"1\"/> </r>") } else { format!("<r b=\"2\" a=\"{i}\"><c n=\"1\"/></r>") }
    };
    let (xa, xb): (Vec<String>, Vec<String>) = (0..n).map(|i| (part(i, false), part(i, true))).unzip();
    let a: Vec<Entry<'_>> = names.iter().zip(&xa).map(|(name, x)| Entry { name, xml: true, bytes: x.as_bytes() }).collect();
    let mut b: Vec<Entry<'_>> = names.iter().zip(&xb).map(|(name, x)| Entry { name, xml: true, bytes: x.as_bytes() }).collect();
    b.reverse();
    let mut work = Work::default();
    let start = Instant::now();
    let d = compare_entries(&a, &b, true, &RULES, &XmlLimits::default(), &mut work).unwrap();
    let time = start.elapsed();
    assert!(d.same(), "{:?}", d.shown.first());
    within("50,000 parts", time, yardstick_time(50_000 * 5));
}
