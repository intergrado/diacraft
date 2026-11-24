//! The journal: the commands that ran, bounded in what it holds, not only in what it shows.

use diacraft_engine::journal::entry_bytes;
use diacraft_engine::{JOURNAL_LIMIT, JOURNAL_MAX_BYTES, JOURNAL_SLACK, Journal};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run};

fn ids(journal: &Journal) -> Vec<&str> {
    journal.entries().iter().map(|(id, _)| id.as_str()).collect()
}

#[test]
fn a_journal_trims_to_its_limit_once_it_is_over_the_limit_plus_the_slack() {
    let mut journal = Journal::with_limits(5, 3, usize::MAX);
    let mut most_held = 0;
    for i in 0..40 {
        journal.push(&format!("c{i}"), json!({}));
        most_held = most_held.max(journal.held_len());
        assert!(journal.held_len() <= 5 + 3, "after {} pushes it holds {}", i + 1, journal.held_len());
        assert_eq!(journal.entries().len(), journal.held_len().min(5), "it shows at most the limit");
        // What it shows is always the newest entries, in order.
        let shown = ids(&journal);
        let expected: Vec<String> = ((i + 1 - 5).max(0)..=i).map(|n| format!("c{n}")).collect();
        assert_eq!(shown, expected.iter().map(String::as_str).collect::<Vec<_>>());
    }
    assert_eq!(most_held, 5 + 3, "it grows to the limit plus the slack, and no further");
    // The ninth entry is the first over: it trims back to the limit, not to the slack.
    let mut journal = Journal::with_limits(5, 3, usize::MAX);
    for i in 0..8 {
        journal.push(&format!("c{i}"), json!({}));
    }
    assert_eq!(journal.held_len(), 8);
    journal.push("c8", json!({}));
    assert_eq!(journal.held_len(), 5);
    assert_eq!(ids(&journal), ["c4", "c5", "c6", "c7", "c8"]);
}

#[test]
fn a_journal_without_slack_never_holds_more_than_the_limit() {
    let mut journal = Journal::with_limits(3, 0, usize::MAX);
    for i in 0..10 {
        journal.push(&format!("c{i}"), json!({}));
        assert!(journal.held_len() <= 3);
    }
    assert_eq!(ids(&journal), ["c7", "c8", "c9"]);
}

/// The memory bound, which `journal()` hides: the held length is checked after every command, through
/// the limit, the slack and past the first trim.
#[test]
fn the_sessions_journal_holds_at_most_the_limit_plus_the_slack_however_long_it_runs() {
    let mut s = blank_session();
    let total = JOURNAL_LIMIT + JOURNAL_SLACK + 500;
    let mut most_held = 0;
    for i in 0..total {
        run(&mut s, "page.rename", json!({"id": 1, "name": format!("n{i}")}));
        let held = s.journal_log().held_len();
        most_held = most_held.max(held);
        assert!(held <= JOURNAL_LIMIT + JOURNAL_SLACK, "after command {i} the journal holds {held} entries");
    }
    assert_eq!(most_held, JOURNAL_LIMIT + JOURNAL_SLACK, "it was allowed its slack, and the trim ran when that was passed");
    assert!(s.journal_log().held_len() < total, "something was let go");
    assert_eq!(s.journal().len(), JOURNAL_LIMIT);
    assert_eq!(check_session(&s), Ok(()));
}

// ---------- the byte budget ----------

/// A value of about `n` bytes of text.
fn text(n: usize) -> Value {
    json!({ "name": "x".repeat(n) })
}

fn total_of_estimates(journal: &Journal) -> usize {
    journal.entries().iter().map(|(id, params)| entry_bytes(id, params, usize::MAX)).sum()
}

#[test]
fn a_journal_drops_the_oldest_entries_to_stay_within_its_byte_budget() {
    // Entries of about 2,000 bytes under a budget of 10,000: a few fit, whatever the count limit allows.
    let mut journal = Journal::with_limits(1_000, 100, 10_000);
    for i in 0..200 {
        journal.push(&format!("c{i}"), text(2_000));
        assert!(journal.held_bytes() <= 10_000, "after {} pushes it holds {} bytes", i + 1, journal.held_bytes());
        assert_eq!(journal.entries().last().map(|e| e.0.clone()), Some(format!("c{i}")), "the newest is always kept");
    }
    assert!((3..=5).contains(&journal.held_len()), "{} entries of ~2 KB under 10 KB", journal.held_len());
    // What it says it holds is what it holds.
    assert_eq!(journal.held_bytes(), total_of_estimates(&journal));
    // It dropped the oldest, in order: the entries kept are the newest ones.
    let kept: Vec<String> = journal.entries().iter().map(|e| e.0.clone()).collect();
    let expected: Vec<String> = (200 - kept.len()..200).map(|i| format!("c{i}")).collect();
    assert_eq!(kept, expected);
}

#[test]
fn the_newest_entry_is_kept_even_when_it_alone_exceeds_the_budget() {
    let mut journal = Journal::with_limits(1_000, 100, 10_000);
    for i in 0..3 {
        journal.push(&format!("small{i}"), text(100));
    }
    journal.push("huge", text(50_000));
    assert_eq!(journal.held_len(), 1, "everything older is let go for it");
    assert_eq!(journal.entries().last().map(|e| e.0.as_str()), Some("huge"));
    assert!(journal.held_bytes() > 10_000, "it is over the budget, and kept");
    // The next entry is the newest and the huge one goes.
    journal.push("small3", text(100));
    assert_eq!(journal.entries().iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["small3"]);
    assert!(journal.held_bytes() <= 10_000);
}

#[test]
fn a_session_journal_keeps_a_newest_entry_larger_than_its_real_budget() {
    let mut journal = Journal::new();
    journal.push("before", json!({}));
    journal.push("huge", text(JOURNAL_MAX_BYTES + (1 << 20)));
    assert_eq!(journal.held_len(), 1);
    assert_eq!(journal.entries().last().map(|e| e.0.as_str()), Some("huge"));
    journal.push("after", json!({}));
    assert_eq!(journal.entries().iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["after"]);
}

#[test]
fn the_budget_is_64_mib() {
    assert_eq!(JOURNAL_MAX_BYTES, 64 * 1024 * 1024);
}

/// The case behind the budget: a legal batch of 256 `page.rename` calls with 255-character names is
/// about 275 KB of JSON, so 11,000 of them would be about 3 GB. Run through real commands, the journal
/// stays under its budget.
#[test]
fn legal_batches_with_long_names_cannot_grow_the_journal_past_its_budget() {
    let mut s = blank_session();
    let name = "\u{1f600}".repeat(255);
    let steps: Vec<Value> = (0..256).map(|_| json!({"command": "page.rename", "params": {"id": 1, "name": name}})).collect();
    let batch = json!({ "commands": steps });
    assert!(batch.to_string().len() > 270_000, "the batch is the size the review measured");

    let mut most_bytes = 0;
    for i in 0..400 {
        run(&mut s, "command.batch", batch.clone());
        let log = s.journal_log();
        most_bytes = most_bytes.max(log.held_bytes());
        assert!(log.held_bytes() <= JOURNAL_MAX_BYTES, "after batch {i} the journal holds {} bytes", log.held_bytes());
        assert_eq!(check_session(&s), Ok(()));
    }
    let log = s.journal_log();
    assert!(log.held_len() < 400 && log.held_len() > 50, "{} entries held", log.held_len());
    assert!(most_bytes > JOURNAL_MAX_BYTES / 2, "the budget was reached, not just never approached");
    assert_eq!(s.journal().last().map(|e| e.0.as_str()), Some("command.batch"), "and the newest entry is there");
}

// ---------- the estimate ----------

/// One leaf of the `BTreeMap` a `serde_json::Map` is built on: room for eleven keys and eleven values and the node's own
/// words, allocated whole for the first entry of an object. 632 bytes on a 64-bit target, as measured in the final review.
const LEAF: usize = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<Value>()) + 16;

/// What a value takes in memory at the least, counted here and not by `entry_bytes`: a node for every value, the text of every
/// string and key, a leaf for every object that has an entry, and one more leaf for every eleven entries after the first.
fn memory_at_least(value: &Value) -> usize {
    let node = std::mem::size_of::<Value>();
    match value {
        Value::String(text) => node + text.len(),
        Value::Array(items) => node + items.iter().map(memory_at_least).sum::<usize>(),
        Value::Object(map) => node + map.len().div_ceil(11) * LEAF + map.iter().map(|(key, v)| key.len() + memory_at_least(v)).sum::<usize>(),
        _ => node,
    }
}

#[test]
fn the_leaf_is_632_bytes_on_a_64_bit_target() {
    #[cfg(target_pointer_width = "64")]
    assert_eq!(LEAF, 632);
}

/// The journal's budget is memory, so the estimate is not the length of the JSON (a string of control characters is six times
/// longer as JSON than in memory, and an object with one entry is a node and a leaf however short its text) but what the value
/// holds: a node for each value, the text of each string and key, and the leaves of its objects.
#[test]
fn the_estimate_covers_what_a_value_holds_in_memory() {
    let long_name = "\u{1f600}".repeat(255);
    let samples = [
        json!({}),
        json!({"id": 1, "name": "Page"}),
        json!({"name": long_name}),
        json!({"commands": [{"command": "page.add"}, {"command": "page.rename", "params": {"id": 1, "name": long_name}}], "label": "A label"}),
        json!([1, 2.5, true, null, "text", [[], [[]]], {"a": {"b": {"c": []}}}]),
        json!("x".repeat(10_000)),
        // Keys count too, though a validated command never has a long one.
        json!({ "k".repeat(5_000): 1, "j".repeat(3_000): {"m".repeat(2_000): []} }),
        // Twelve entries need a second leaf.
        Value::Object((0..12).map(|i| (format!("key{i}"), json!(i))).collect()),
    ];
    for params in samples {
        let estimate = entry_bytes("command.batch", &params, usize::MAX);
        let held = "command.batch".len() + memory_at_least(&params);
        assert!(estimate >= held, "{estimate} bytes estimated for a value that holds at least {held}: {params}");
    }
}

/// A non-empty object is charged a leaf and an empty one nothing, whatever its text: the case that put the estimate six times
/// under the memory, a batch of 512 small objects.
#[test]
fn a_non_empty_object_is_charged_a_leaf_and_an_empty_one_is_not() {
    let estimate = |value: Value| entry_bytes("x", &value, usize::MAX);
    // `{"a":1}` and `[1]` have a node for the container and one for the 1: the object also has a key, its slot and a leaf.
    assert!(estimate(json!([{"a": 1}])) >= estimate(json!([[1]])) + LEAF, "an object with one entry costs a leaf");
    assert_eq!(estimate(json!([{}])), estimate(json!([[]])), "an empty object allocates nothing");
    // 256 steps of two small objects each: what a batch is.
    let step = json!({"command": "page.rename", "params": {"id": 1, "name": "a"}});
    let batch = json!({ "commands": vec![step; 256] });
    assert!(estimate(batch) >= 513 * LEAF, "513 non-empty objects hold at least 513 leaves");
    // Eleven entries share one leaf, the twelfth needs another.
    let object = |n: usize| Value::Object((0..n).map(|i| (format!("k{i:02}"), Value::Null)).collect());
    let (eleven, twelve) = (estimate(object(11)), estimate(object(12)));
    assert!(twelve >= eleven + LEAF, "{eleven} for eleven entries and {twelve} for twelve");
    let one_more_entry = estimate(object(2)) - estimate(object(1));
    assert_eq!(estimate(object(11)) - estimate(object(1)), 10 * one_more_entry, "the second to the eleventh entry cost no more leaves");
}

/// The estimate is not the length of the JSON, and the journal's comment once said it never fell short of it.
#[test]
fn a_string_of_control_characters_is_longer_as_json_than_the_estimate() {
    let params = Value::String("\u{1}".repeat(10_000));
    let json_len = params.to_string().len();
    let estimate = entry_bytes("x", &params, usize::MAX);
    assert!(json_len > 60_000 && estimate < json_len, "{estimate} estimated, {json_len} bytes of JSON");
}

#[test]
fn the_estimate_stops_at_its_cap_and_does_not_recurse() {
    // A million nodes with a cap of 1,000 bytes: it stops after a few dozen of them.
    let wide = Value::Array(vec![Value::Null; 1_000_000]);
    let estimate = entry_bytes("x", &wide, 1_000);
    assert!(estimate > 1_000 && estimate < 2_000, "{estimate}");
    // Deep nesting is walked with a stack of its own, not by recursion.
    let mut deep = json!("core");
    for _ in 0..2_000 {
        deep = Value::Array(vec![deep]);
    }
    assert!(entry_bytes("x", &deep, usize::MAX) > 2_000 * 32);
}

#[test]
fn an_entry_far_over_the_budget_is_recorded_as_over_it_without_being_walked_to_the_end() {
    let mut journal = Journal::with_limits(10, 0, 1_000);
    journal.push("wide", Value::Array(vec![Value::Null; 1_000_000]));
    assert_eq!(journal.held_len(), 1);
    assert!(journal.held_bytes() > 1_000 && journal.held_bytes() < 2_000, "{}", journal.held_bytes());
}
