//! A refusal is not an abort (done-criterion D3): the content check's claim that a part past the XML limits is an error in words, and not a
//! crash, run in a child process under `ulimit -v 3000000` (an address space of 2.86 GiB). The pattern is `crates/engine/tests/it/odg_memory.rs`:
//! the child is `#[ignore]` and does the work, and each parent runs it with one scenario and reads what it printed.
//!
//! What the check does not claim, and these tests do not test: a pair of parts *within* the limits that is more than the machine holds (a pair
//! at the item limit holds 3 GB) is not refused, since the trees are built with ordinary allocation, as the parser's are.

use std::process::Command;

use diacraft_container::canon::{Entry, Name, ODF_TEXT_NAMESPACE, QKind, Qualified, Rules, compare_entries};
use diacraft_container::{Work, XmlLimits};

const SCENARIO: &str = "CANON_MEMORY_SCENARIO";

/// Paragraph text: `text:p` is paragraph content.
const PARAGRAPHS: Rules = Rules {
    keyed: &[],
    numbers: &[],
    lengths: &[],
    qnames: &[],
    paragraphs: &[Name { ns: ODF_TEXT_NAMESPACE, local: "p" }],
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

/// A document of one paragraph that holds `body`.
fn paragraph(body: &str) -> String {
    format!("<office:text xmlns:office=\"urn:o\" xmlns:text=\"{ODF_TEXT_NAMESPACE}\"><text:p>{body}</text:p></office:text>")
}

/// The comparison of two documents under the paragraph rules.
fn compare_paragraphs(a: &str, b: &str) -> (String, Work) {
    let mut work = Work::default();
    let (ea, eb) = (Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }, Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() });
    let result = compare_entries(&[ea], &[eb], true, &PARAGRAPHS, &XmlLimits::default(), &mut work);
    (result.map(|d| format!("compared count={}", d.count)).unwrap_or_else(|e| e.class().to_owned()), work)
}

/// `n` elements of two attributes each under a root: `3n + 1` items.
fn items(n: usize) -> String {
    let mut s = String::with_capacity(n * 20 + 16);
    s.push_str("<r>");
    for _ in 0..n {
        s.push_str("<e a=\"1\" b=\"2\"/>");
    }
    s.push_str("</r>");
    s
}

fn compare(a: &str, b: &str) -> (String, Work) {
    let mut work = Work::default();
    let (ea, eb) = (Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }, Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() });
    let result = compare_entries(&[ea], &[eb], true, &Rules::NONE, &XmlLimits::default(), &mut work);
    (result.map(|d| format!("compared count={}", d.count)).unwrap_or_else(|e| e.class().to_owned()), work)
}

/// The XML events of reading `text` alone, which is what a refusal that comes before its partner is read costs.
fn events_of(text: &str) -> u64 {
    let mut work = Work::default();
    let limits = XmlLimits { max_nodes: diacraft_container::canon::MAX_COMPARE_ITEMS, ..XmlLimits::default() };
    let _ = diacraft_container::xml::parse_counted(text.as_bytes(), &limits, &mut work);
    work.xml_events
}

#[test]
#[ignore = "run by the tests below under `ulimit -v`; alone it needs the scenario in the environment"]
fn child() {
    let scenario = std::env::var(SCENARIO).expect("run by a parent test, with the scenario in the environment");
    match scenario.as_str() {
        // One side at the item budget (666,666 elements and the root: 1,999,999 of the 2,000,000 items) and the other past it.
        "item_limit_then_past" => {
            let (at, past) = (items(666_666), items(666_667));
            let (said, _) = compare(&at, &past);
            println!("\nRESULT {said}");
        }
        // The first side is past the limit and the second is at it: the first is refused before the second is read.
        "past_then_item_limit" => {
            let (past, at) = (items(666_667), items(666_666));
            let (said, work) = compare(&past, &at);
            println!("\nRESULT {said} events={} alone={}", work.xml_events, events_of(&past));
        }
        // An attribute of 2 MiB where the limit is 1 MiB, and a tree 20,000 deep where the limit is 200.
        "attribute_and_depth" => {
            let big = format!("<r a=\"{}\"/>", "x".repeat(2 << 20));
            let deep = format!("{}{}", "<k>".repeat(20_000), "</k>".repeat(20_000));
            println!("\nRESULT attribute={} depth={}", compare(&big, "<r/>").0, compare("<r/>", &deep).0);
        }
        // A paragraph of 32 MiB of words a space apart on each side, which differ in the last letter: the text of a paragraph is one string
        // as long as the text, and not a token for each word and each run of spaces.
        "paragraph_32_mib" => {
            let words = "a ".repeat(16 << 20);
            let (a, b) = (paragraph(&format!("{words}x")), paragraph(&format!("{words}y")));
            let (said, _) = compare_paragraphs(&a, &b);
            println!("\nRESULT {said}");
        }
        // A prefix bound to a URI of 500,000 bytes, used 100,000 times in a list of prefixes (a value of 200 KB): expanded, it would be 50 GB.
        "qualified_long_uri" => {
            let rules =
                Rules { qnames: &[Qualified { attr: Name { ns: "urn:m", local: "Ignorable" }, on: None, kind: QKind::Prefixes }], ..Rules::NONE };
            let document = |extra: &str| {
                format!("<r xmlns:m=\"urn:m\" xmlns:p=\"{}\" m:Ignorable=\"{}\"{extra}/>", "u".repeat(500_000), vec!["p"; 100_000].join(" "))
            };
            let (a, b) = (document(""), document(" z=\"1\""));
            let mut work = Work::default();
            let (ea, eb) = (Entry { name: "/p.xml", xml: true, bytes: a.as_bytes() }, Entry { name: "/p.xml", xml: true, bytes: b.as_bytes() });
            let result = compare_entries(&[ea], &[eb], true, &rules, &XmlLimits::default(), &mut work);
            let said = result.map(|d| format!("compared count={}", d.count)).unwrap_or_else(|e| format!("{} {e}", e.class()));
            println!("\nRESULT {said} counted={}", work.canon_bytes);
        }
        other => panic!("no scenario {other}"),
    }
}

/// Runs the child's `scenario` under `ulimit -v limit_kib`: whether it ended well, and what it printed.
fn run_child(scenario: &str, limit_kib: u64) -> (bool, String) {
    let exe = std::env::current_exe().expect("the path of this test binary");
    let script = format!("ulimit -v {limit_kib} && exec \"$0\" --exact canon_memory::child --ignored --nocapture --test-threads=1");
    let out = Command::new("/bin/sh").arg("-c").arg(script).arg(exe).env(SCENARIO, scenario).output().expect("run /bin/sh");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    if !out.status.success() {
        eprintln!("child `{scenario}` under {limit_kib} KiB ended with {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status);
    }
    (out.status.success(), stdout)
}

fn result_line(stdout: &str) -> String {
    stdout.lines().find(|l| l.starts_with("RESULT")).unwrap_or("no RESULT line").to_owned()
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_part_at_the_item_limit_against_one_past_it_is_refused_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("item_limit_then_past", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_line(&stdout), "RESULT xml_too_complex");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_part_past_the_item_limit_is_refused_before_its_partner_is_read() {
    let (ended_well, stdout) = run_child("past_then_item_limit", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    // The events are those of reading the first part alone: the second was never read.
    let words: Vec<&str> = line.split_whitespace().collect();
    let (events, alone) = (words.get(2).and_then(|w| w.strip_prefix("events=")), words.get(3).and_then(|w| w.strip_prefix("alone=")));
    assert_eq!(words.get(1), Some(&"xml_too_complex"), "{line}");
    assert!(events.is_some() && events == alone, "{line}");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_part_with_an_attribute_or_a_depth_past_the_limits_is_refused_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("attribute_and_depth", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_line(&stdout), "RESULT attribute=xml_too_complex depth=xml_too_complex");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_paragraph_of_32_mib_of_words_is_compared_under_the_address_limit_and_does_not_abort() {
    let (ended_well, stdout) = run_child("paragraph_32_mib", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    assert_eq!(result_line(&stdout), "RESULT compared count=1");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS cannot limit a process's address space (`ulimit -v` fails there), and this test runs its child under that limit"
)]
fn a_prefix_bound_to_a_long_uri_and_used_100000_times_is_refused_in_words_and_does_not_abort() {
    let (ended_well, stdout) = run_child("qualified_long_uri", 3_000_000);
    assert!(ended_well, "the child aborted, or its assertion failed: {stdout}");
    let line = result_line(&stdout);
    assert!(line.starts_with("RESULT xml_too_complex \"/p.xml\": "), "{line}");
    // What was counted is the two documents and the limit at the most, not the 50 GB.
    let counted: u64 = line.rsplit("counted=").next().and_then(|n| n.parse().ok()).expect("a count");
    assert!(counted < 17 << 20, "{counted}");
}
