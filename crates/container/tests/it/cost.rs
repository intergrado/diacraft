//! What hostile archives cost, counted in the units each loop walks ([`Work`]) and never timed: a count does not move with the load on the
//! machine, and a count that does not grow with the file is a claim a time cannot make.
//!
//! The shapes are *built*, one test for each: random mutation of a valid archive (the fuzz tests) never makes an archive of fifteen
//! hundred nested end records or a directory of rows full of empty extra records, and those are what the ZIP library this reader
//! replaced spent minutes and hundreds of megabytes on. They are carried over from the `opc` crate's tests
//! (`directory.rs` and `memory.rs`, which time them and measure their memory through `opc` and still do), here as counts.

use diacraft_container::xml::{XmlLimits, parse_counted};
use diacraft_container::zip::{Archive, Plain};
use diacraft_container::{Limits, Work};

use crate::zipbuild::{Item, Opts, build, close, many_entries};

/// The window of the backward scan for the end record: its 22 bytes and the longest comment, 65,535 bytes, give 65,536 places to start.
const WINDOW: u64 = 65_536;

/// The class reading `bytes` with `limits` gives (or `"read"`), and what the read cost.
fn counted(bytes: &[u8], limits: &Limits) -> (&'static str, Work) {
    let mut work = Work::default();
    let class = match Archive::read_with(bytes, limits, &mut Plain::new(limits), &mut work) {
        Ok(_) => "read",
        Err(e) => e.class(),
    };
    (class, work)
}

/// `n` bytes of ZIP64 end record signatures, eight bytes at a time.
fn zip64_signatures(n: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(n);
    for _ in 0..n / 8 {
        bytes.extend_from_slice(b"PK\x06\x06\x2c\0\0\0");
    }
    bytes
}

/// `zip64_signatures(megabytes)` and then `records` end records, each with a comment length of one byte that the next record's first
/// byte is: none of them ends the file, and all but the last fit inside it.
fn signature_tail(megabytes: usize, records: usize) -> Vec<u8> {
    let mut bytes = zip64_signatures(megabytes << 20);
    for _ in 0..records {
        bytes.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x00");
    }
    bytes
}

#[test]
fn the_end_record_signature_tail_costs_one_window_whatever_the_file_holds() {
    // Review finding 1, the shape as described: 1 MB of ZIP64 end record signatures and a tail of 10,000 end record signatures, none
    // of which ends the file. The last but one fits (its comment of one byte is inside the file), so it is the end record,
    // and its numbers reach nothing: `zip_corrupt`. The scan looks at every place in its window once, to see whether a record that ends the
    // file is before the one that fits, and no further.
    let (class, work) = counted(&signature_tail(1, 10_000), &Limits::default());
    assert_eq!(class, "zip_corrupt");
    assert_eq!(work.end_scan, WINDOW, "one pass of the window");
    assert_eq!((work.rows, work.extra_records, work.local_headers, work.inflated), (0, 0, 0, 0), "{work:?}");
    // Twice the file, twice the records: the same window, the same count.
    let (class, twice) = counted(&signature_tail(2, 20_000), &Limits::default());
    assert_eq!((class, twice), ("zip_corrupt", work));
    // Starting like a ZIP changes what a file with no record is called, and not what the scan costs.
    let mut starts_like_a_zip = b"PK\x03\x04".to_vec();
    starts_like_a_zip.extend(signature_tail(1, 10_000));
    assert_eq!(counted(&starts_like_a_zip, &Limits::default()), ("zip_corrupt", work));
    // A file shorter than the window costs the places it has: an end record's 22 bytes need 22 bytes.
    let (class, short) = counted(&signature_tail(0, 100), &Limits::default());
    assert_eq!((class, short.end_scan), ("zip_corrupt", 100 * 22 - 22 + 1));
    // No record that fits at all: `not_zip` after the same one window.
    let (class, none) = counted(&zip64_signatures(1 << 20), &Limits::default());
    assert_eq!((class, none.end_scan), ("not_zip", WINDOW));
}

/// End records nested so that every one has a comment that reaches the end of the file, each behind a ZIP64 locator, after `megabytes` of
/// ZIP64 record signatures: every one is "the end record" by the comment rule, and every one points at a ZIP64 record that is one of the
/// signatures. The reader this crate's replaced retried every signature for every record (26.5 s for 1,500 records behind 9 MB).
fn nested_end_records_with_locators(records: usize, megabytes: usize) -> Vec<u8> {
    let mut bytes = zip64_signatures(megabytes << 20);
    let mut tail: Vec<u8> = Vec::new();
    for _ in 0..records {
        let mut record = b"PK\x06\x07\0\0\0\0".to_vec();
        record.extend_from_slice(&5u64.to_le_bytes());
        record.extend_from_slice(&1u32.to_le_bytes());
        record.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff");
        record.extend_from_slice(&(tail.len() as u16).to_le_bytes());
        record.extend_from_slice(&tail);
        tail = record;
    }
    bytes.extend_from_slice(&tail);
    bytes
}

#[test]
fn nested_end_records_behind_zip64_locators_cost_one_look_and_no_second_try() {
    // The innermost record ends the file (its comment is empty), so the backward scan finds it at its first place; its locator points at a
    // place that holds no ZIP64 record, and that is the answer: no other record and no other signature is tried.
    let bytes = nested_end_records_with_locators(1500, 9);
    assert!(bytes.len() > 9_000_000);
    let (class, work) = counted(&bytes, &Limits::default());
    assert_eq!(class, "zip_corrupt");
    assert_eq!(work.end_scan, 1);
    assert_eq!(work.rows, 0);
    // A single end record that ends the file and leaves everything to a ZIP64 record its locator puts inside the signatures.
    let mut ends = zip64_signatures(1 << 20);
    ends.extend_from_slice(b"PK\x06\x07\0\0\0\0");
    ends.extend_from_slice(&7u64.to_le_bytes());
    ends.extend_from_slice(&1u32.to_le_bytes());
    ends.extend_from_slice(b"PK\x05\x06\0\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0");
    assert_eq!(counted(&ends, &Limits::default()).0, "zip_corrupt");
    assert_eq!(counted(&ends, &Limits::default()).1.end_scan, 1);
}

#[test]
fn four_megabytes_of_row_signatures_cost_one_row_and_no_name() {
    // An end record that says a directory of four megabytes of row signatures holds 1,000 rows: the first row's fields are the signature
    // over and over, a name length of 19,280, far past three times the name limit, refused before a byte of it is copied.
    let mut bytes = Vec::new();
    for _ in 0..(4 << 20) / 4 {
        bytes.extend_from_slice(b"PK\x01\x02");
    }
    let len = bytes.len() as u64;
    close(&mut bytes, &Opts::default(), 1000, len, 0);
    let (class, work) = counted(&bytes, &Limits::default());
    assert_eq!(class, "invalid_part_name");
    assert_eq!((work.rows, work.name_bytes, work.extra_records), (1, 0, 0), "{work:?}");
}

#[test]
fn many_empty_extra_fields_are_each_walked_once_and_none_kept() {
    // Review finding 3: 1,000 rows with 64 KB of empty extra records each (a file of 64 MB) took 690 MB with the library this
    // reader replaced. Each row here has 65,480 bytes of empty records, 16,370 of them, and the walk visits each once: twice the rows is
    // twice the records, exactly. (The `opc` crate's `memory.rs` reads the 64 MB file under an address-space limit, and finds nothing kept.)
    const RECORDS_PER_ROW: u64 = 65_480 / 4;
    let rows = |n: usize| many_entries((0..n).map(|i| format!("p{i}").into_bytes()), 65_480);
    let (class, hundred) = counted(&rows(100), &Limits::default());
    assert_eq!(class, "read");
    assert_eq!((hundred.rows, hundred.extra_records), (100, 100 * RECORDS_PER_ROW));
    let (class, two_hundred) = counted(&rows(200), &Limits::default());
    assert_eq!(class, "read");
    assert_eq!((two_hundred.rows, two_hundred.extra_records), (200, 200 * RECORDS_PER_ROW));
    // The extra fields are walked for what they hold, and nothing of them is in what is kept.
    let bytes = rows(100);
    let archive = Archive::read(&bytes, &Limits::default()).unwrap();
    assert_eq!(archive.entries().len(), 100);
    assert!(archive.entries().iter().all(|e| e.name.len() <= 4 && e.written.is_none()));
}

#[test]
fn the_row_limit_with_the_longest_names_costs_each_name_once() {
    // The rows a count may say are twice the part limit and sixteen. With a part limit of 1,000: 1,000 parts and 1,016 folders, each name
    // the longest a name may be (1,024 bytes, a folder's slash apart). Each name is read once; one row more is refused from the count, before
    // any row is read.
    let limits = Limits { max_parts: 1000, ..Limits::default() };
    let rows_allowed = 2 * 1000 + 16;
    let names = |rows: usize| {
        let parts = (0..1000).map(|i| format!("{}{i:08}", "f".repeat(1024 - 8)).into_bytes());
        let folders = (0..rows - 1000).map(|i| format!("{}{i:08}/", "d".repeat(1024 - 8)).into_bytes());
        parts.chain(folders).collect::<Vec<_>>()
    };
    let at_the_limit = names(rows_allowed);
    let name_bytes: u64 = at_the_limit.iter().map(|n| n.len() as u64).sum();
    let (class, work) = counted(&many_entries(at_the_limit.into_iter(), 0), &limits);
    assert_eq!(class, "read");
    assert_eq!((work.rows, work.name_bytes), (rows_allowed as u64, name_bytes));
    // One local header for each row, and one more for each entry kept (folders are kept by the plain judge).
    assert_eq!(work.local_headers, 2 * rows_allowed as u64);
    let (class, over) = counted(&many_entries(names(rows_allowed + 1).into_iter(), 0), &limits);
    assert_eq!(class, "too_many_parts");
    assert_eq!((over.rows, over.name_bytes, over.local_headers), (0, 0, 0), "refused from the count: {over:?}");
    // A name of three times the limit and one byte is refused before it is copied. One of three times the limit is copied (a caller's
    // name may shrink to a third when its escapes are decoded), and then the plain judge refuses it for its length.
    let (class, long) = counted(&many_entries(std::iter::once(vec![b'n'; 3 * 1024 + 1]), 0), &limits);
    assert_eq!((class, long.rows, long.name_bytes), ("invalid_part_name", 1, 0));
    let (class, at_three) = counted(&many_entries(std::iter::once(vec![b'n'; 3 * 1024]), 0), &limits);
    assert_eq!((class, at_three.rows, at_three.name_bytes), ("invalid_part_name", 1, 3 * 1024));
}

/// `n` names of 1,024 bytes that share their first 1,016, in an order a sort has to work at (`i * 7,919` modulo `n`: a permutation, since
/// 7,919 is a prime that divides no `n` used here).
fn long_shared_names(n: usize) -> Vec<Vec<u8>> {
    (0..n).map(|i| format!("{}{:08}", "f".repeat(1016), (i * 7919) % n).into_bytes()).collect()
}

/// `n * ceil(log2 n)`.
fn n_log_n(n: u64) -> u64 {
    n * u64::from(64 - (n - 1).leading_zeros())
}

#[test]
fn the_plain_judges_duplicate_rule_costs_a_sort_of_the_names_and_one_pass() {
    // The duplicate rule sorts every name kept (and every name as written that a field replaced) and walks the
    // neighbours once. With names that share a kilobyte, each comparison looks at a kilobyte, so their number is what has to stay at
    // n log n and not grow as n squared. Counted, in comparisons of two names.
    let limits = Limits { max_parts: 20_000, ..Limits::default() };
    let comparisons = |n: usize| {
        let (class, work) = counted(&many_entries(long_shared_names(n).into_iter(), 0), &limits);
        assert_eq!(class, "read", "{n} names");
        work.name_comparisons
    };
    let (small, large) = (comparisons(4_000), comparisons(16_000));
    eprintln!("COUNTED name comparisons: {small} for 4,000 names, {large} for 16,000");
    // At least the pass over the neighbours, and at most twice n log n with the pass.
    assert!((3_999..=2 * n_log_n(4_000) + 4_000).contains(&small), "{small} comparisons for 4,000 names");
    assert!((15_999..=2 * n_log_n(16_000) + 16_000).contains(&large), "{large} comparisons for 16,000 names");
    // Four times the names cost about four times the comparisons and a little more (n log n), and nowhere near sixteen times (n squared).
    assert!(large < 5 * small, "{small} for 4,000 names and {large} for 16,000");
    // A duplicate is found by the same sort and refused: the count stops at the pass that finds it.
    let mut twice = long_shared_names(1000);
    twice.push(twice[500].clone());
    let (class, work) = counted(&many_entries(twice.into_iter(), 0), &limits);
    assert_eq!(class, "duplicate_part");
    assert!(work.name_comparisons > 0 && work.name_comparisons <= 2 * n_log_n(1001) + 1001, "{work:?}");
    // A refusal before the list is judged costs no comparison.
    let (class, none) = counted(&many_entries(long_shared_names(20).into_iter().chain([b"../x".to_vec()]), 0), &limits);
    assert_eq!((class, none.name_comparisons), ("path_traversal", 0));
}

#[test]
fn an_entry_that_holds_more_than_it_may_costs_its_limit_and_one_byte() {
    // A deflated megabyte of zeros whose header says 4 KiB, under a part cap of 4 KiB: the inflater stops one byte past the cap and the
    // entry is refused from what came out; the rest of the megabyte is never inflated.
    let limits = Limits { max_part_bytes: 4096, max_ratio: 2000, ..Limits::default() };
    let bytes = build(&[Item::deflated("big.bin", &vec![0u8; 1 << 20]).claiming_size(4096)]);
    let mut archive = Archive::read(&bytes, &limits).unwrap();
    let mut work = Work::default();
    assert_eq!(archive.inflate(0, &mut work).unwrap_err().class(), "part_too_large");
    assert_eq!(work.inflated, 4097);
    // The same past the size of the buffer the bytes are read through (64 KiB): a stored megabyte whose row says 100,000 bytes, under a cap
    // of 100,000, is read to 100,001 bytes and no further.
    let limits = Limits { max_part_bytes: 100_000, ..Limits::default() };
    let bytes = build(&[Item::stored("big.bin", &vec![7u8; 1 << 20]).claiming_size(100_000)]);
    let mut archive = Archive::read(&bytes, &limits).unwrap();
    let mut work = Work::default();
    assert_eq!(archive.inflate(0, &mut work).unwrap_err().class(), "part_too_large");
    assert_eq!(work.inflated, 100_001);
    // An honest entry costs its own bytes.
    let honest = build(&[Item::deflated("a.xml", &b"<a/>".repeat(1000))]);
    let mut archive = Archive::read(&honest, &Limits::default()).unwrap();
    let mut work = Work::default();
    assert_eq!(archive.inflate(0, &mut work).unwrap().len(), 4000);
    assert_eq!(work.inflated, 4000);
}

#[test]
fn an_xml_tree_costs_an_event_for_each_thing_in_it_and_a_refusal_costs_what_came_before() {
    let limits = XmlLimits::default();
    let events = |text: &str| {
        let mut work = Work::default();
        let class = parse_counted(text.as_bytes(), &limits, &mut work).map(|_| "parsed").unwrap_or_else(|e| e.class());
        (class, work.xml_events)
    };
    // The root's start and end, one event for each child, and the end of the input.
    let doc = |children: usize| format!("<a>{}</a>", "<b/>".repeat(children));
    assert_eq!(events(&doc(10)), ("parsed", 13));
    assert_eq!(events(&doc(20)), ("parsed", 23));
    // A megabyte of white space before the root is one event of text.
    assert_eq!(events(&format!("{}<a/>", " ".repeat(1 << 20))), ("parsed", 3));
    // A DTD is refused at the first event, before anything in it is read or expanded.
    assert_eq!(events("<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>"), ("xml_entity_refused", 1));
    // Past the item limit the parse stops where the limit is.
    let tight = XmlLimits { max_nodes: 5, ..XmlLimits::default() };
    let mut work = Work::default();
    assert_eq!(parse_counted(doc(1000).as_bytes(), &tight, &mut work).unwrap_err().class(), "xml_too_complex");
    assert_eq!(work.xml_events, 6);
}
