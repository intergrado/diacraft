//! The security scan of `ext/odf14.rs` (three MEDIUM findings): what an earlier ODF version's style forms cost a save and a read.
//!
//! 1. **The names given to unnamed list styles.** A style of 100,000 list styles with no name: the writer looked each name it tried up in a list (quadratic), uncharged. Now the names are a
//!    set, the walks are charged to the save's steps before they are made, and a style of more records than a save carries (4,096) is refused in words.
//! 2. **The records a document file holds for a style.** 100,000 `older` records of properties left out: the reader looked each up in the style's properties as it pushed them (quadratic), and
//!    kept every one. Now each record takes an item before it is kept, at most 4,096 are kept for a style, only a property ODF 1.4 refuses is kept, the first of each key, and the look-up is a set.
//! 3. **The restore of the kept tree.** A kept tree of 100,000 nodes and 10,000 records of label widths: the reader looked each record up in a list at each node, uncharged. Now the records are
//!    maps, the tree's nodes are charged before the walk that makes it anew, and no walk is made when no list style of the tree is one a record names.
//!
//! Each case is timed against a plain yardstick of the same size (the same file with nothing in an earlier form, read or saved the same way), and runs in a thread that the test waits for only
//! so long: a quadratic build is a test that fails, not one that hangs.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use diacraft_odg::{Limits, ReadReport, WriteError, WriteOptions, WriteWork, read, write_counted};

use crate::docfile_cost::file_of;
use crate::drawing::Drawing;
use crate::write_core::custom;

/// The path of DiaCraft's document file in a package.
const DOCFILE: &str = "DiaCraft/document.xml";

/// A drawing whose common style `Plain` holds `lists` in its graphic properties, with the document file `docfile` (none when empty).
fn drawing(lists: &str, docfile: &str) -> Vec<u8> {
    let style = format!(
        r#"<style:style style:name="Plain" style:family="graphic"><style:graphic-properties draw:fill="none">{lists}</style:graphic-properties></style:style>"#
    );
    let mut d = Drawing::a4().common(&style).page(&custom("s1", "1cm"));
    if !docfile.is_empty() {
        d.files.push((DOCFILE.to_owned(), docfile.as_bytes().to_vec()));
    }
    d.bytes()
}

/// Runs `job` in a thread of its own and gives what it gives and how long it took, or panics when it is still running after `most` (a quadratic job fails the test, and its thread is left to
/// end with the process).
fn within<T: Send + 'static>(what: &str, most: Duration, job: impl FnOnce() -> T + Send + 'static) -> (T, Duration) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let started = Instant::now();
        let out = job();
        let _ = tx.send((out, started.elapsed()));
    });
    rx.recv_timeout(most).unwrap_or_else(|_| panic!("{what}: still running after {most:?}"))
}

/// How long `job` takes, at its best of three runs (the yardstick).
fn best_of_three(mut job: impl FnMut()) -> Duration {
    (0..3)
        .map(|_| {
            let started = Instant::now();
            job();
            started.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// The time a case may take: a small multiple of its yardstick, and a floor for a loaded machine.
fn allowed(yardstick: Duration) -> Duration {
    yardstick * 6 + Duration::from_millis(1_500)
}

/// The model writer's save of `bytes` read, with what it charged.
fn saved(bytes: &[u8]) -> (Result<diacraft_odg::Written, WriteError>, WriteWork) {
    let (doc, _) = read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("not read: {e}"));
    let mut work = WriteWork::default();
    let written = write_counted(&doc, None, &WriteOptions::default(), &mut work);
    (written, work)
}

/// Finding 1: **100,000 list styles with no name** in one style. The save is refused in words (more than a save carries for a style), after a charge of at least the style's nodes, in a time of
/// the order of the save of the same style whose list styles are named (which writes them all). Red on the build the scan read: the names were looked up in a list, and the save ran for minutes.
#[test]
fn a_style_of_a_hundred_thousand_unnamed_list_styles_is_refused_in_words_in_linear_time() {
    const N: usize = 100_000;
    let unnamed = drawing(&"<text:list-style/>".repeat(N), "");
    let named = drawing(&(0..N).map(|k| format!(r#"<text:list-style style:name="L{k}"/>"#)).collect::<String>(), "");
    let yardstick = best_of_three(|| {
        let (written, _) = saved(&named);
        assert!(written.is_ok(), "the yardstick is saved: {:?}", written.err());
    });
    let ((written, work), took) = within("the save of 100,000 unnamed list styles", allowed(yardstick), move || saved(&unnamed));
    match written {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("4,096 list styles with no name"), "{words}"),
        other => panic!("not refused in words: {:?}", other.map(|w| w.bytes.len())),
    }
    assert!(work.steps_used() >= N as u64, "the style's nodes are charged: {}", work.steps_used());
    assert!(took <= allowed(yardstick), "took {took:?} against the yardstick's {yardstick:?}");
}

/// The read of `bytes`, which must succeed.
fn read_ok(bytes: &[u8]) -> ReadReport {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("not read: {e}")).1
}

/// The unmapped properties of the style `Plain` that a read of `bytes` gives.
fn plain_unmapped(bytes: &[u8]) -> Vec<(String, String)> {
    let (doc, _) = read(bytes, &Limits::default()).unwrap();
    let style = doc.styles.iter().find(|s| s.ext.get().and_then(|d| d.odf.name.as_deref()) == Some("Plain")).expect("the style Plain").clone();
    style.ext.get().unwrap().odf.unmapped.iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// Finding 2: **a style's entry of 100,000 records of properties left out**, each of a key of its own that ODF 1.4 refuses (`text:enable-numbering` under another prefix each time). The read keeps
/// at most 4,096 for the style, takes an item for each record it reads, and takes a time of the order of the read of the same entry whose children are of a kind the reader skips. Red on the build
/// the scan read: every record was looked up in the properties it had pushed, and the read ran for minutes.
#[test]
fn a_style_entry_of_a_hundred_thousand_records_is_read_to_its_cap_in_linear_time() {
    const N: usize = 100_000;
    let records: String =
        (0..N).map(|k| format!(r#"<diacraft:older key="p{k}:paragraph-properties/text:enable-numbering" value="false"/>"#)).collect();
    let skipped: String =
        (0..N).map(|k| format!(r#"<diacraft:elder key="p{k}:paragraph-properties/text:enable-numbering" value="false"/>"#)).collect();
    let held = drawing("", &file_of(&format!(r#"<diacraft:style name="Plain">{records}</diacraft:style>"#)));
    let plain = drawing("", &file_of(&format!(r#"<diacraft:style name="Plain">{skipped}</diacraft:style>"#)));
    let base = read_ok(&plain).work.items;
    let yardstick = best_of_three(|| {
        read_ok(&plain);
    });
    let held_copy = held.clone();
    let (report, took) = within("the read of 100,000 records", allowed(yardstick), move || read_ok(&held_copy));
    assert!(took <= allowed(yardstick), "took {took:?} against the yardstick's {yardstick:?}");
    let kept = plain_unmapped(&held);
    assert_eq!(kept.len(), 4_096, "the first 4,096 are kept");
    assert!(report.work.items >= base + 2 * 4_096, "an item for each record read and each property kept: {} against {base}", report.work.items);
    assert!(report.warning_counts.contains_key("diacraft_older_too_many"), "{:?}", report.warning_counts);
    // A record of a property ODF 1.4 takes is none of what the entry gives back, and of two records of one key the first is kept.
    let odd = drawing(
        "",
        &file_of(concat!(
            r#"<diacraft:style name="Plain"><diacraft:older key="style:graphic-properties/draw:fill" value="solid"/>"#,
            r#"<diacraft:older key="style:paragraph-properties/text:enable-numbering" value="false"/><diacraft:older key="style:paragraph-properties/text:enable-numbering" value="true"/></diacraft:style>"#
        )),
    );
    assert_eq!(plain_unmapped(&odd), vec![("style:paragraph-properties/text:enable-numbering".to_owned(), "false".to_owned())]);
    // A second entry for the same style puts nothing back (its records would be read again over the first's), and says so once.
    let one = r#"<diacraft:style name="Plain"><diacraft:older key="a:paragraph-properties/text:enable-numbering" value="false"/></diacraft:style>"#;
    let other = r#"<diacraft:style name="Plain"><diacraft:older key="b:paragraph-properties/text:enable-numbering" value="true"/></diacraft:style>"#;
    let twice = drawing("", &file_of(&format!("{one}{other}")));
    assert_eq!(plain_unmapped(&twice), vec![("a:paragraph-properties/text:enable-numbering".to_owned(), "false".to_owned())]);
    assert!(read_ok(&twice).warning_counts.contains_key("diacraft_older_twice"));
}

/// The list styles of finding 3: `n` named `L0` and up, each of three levels whose label width is 0 (with a text and three other elements: ten nodes a list style).
fn level_lists(n: usize) -> String {
    let level = |l: usize| {
        format!(
            r#"<text:list-level-style-bullet text:level="{l}"><style:list-level-properties text:min-label-width="0cm"/></text:list-level-style-bullet>"#
        )
    };
    (0..n)
        .map(|k| format!(r#"<text:list-style style:name="L{k}">{}{}{}t<draw:a/><draw:b/><draw:c/></text:list-style>"#, level(1), level(2), level(3)))
        .collect()
}

/// Finding 3: **a kept tree of 100,000 nodes and 10,000 records of label widths.** The read gives the widths back by maps, charges the tree's nodes before the walk that makes it anew, and takes a
/// time of the order of the read of the same tree with no records; a record that names no list style of the tree makes no walk. Red on the build the scan read: each node looked each record up in a
/// list.
#[test]
fn a_kept_tree_of_a_hundred_thousand_nodes_and_ten_thousand_records_is_restored_by_maps_and_charged() {
    const LISTS: usize = 10_000;
    let lists = level_lists(LISTS);
    let records: String = (0..LISTS).map(|k| format!(r#"<diacraft:older list="L{k}" level="2" width="-1cm"/>"#)).collect();
    let held = drawing(&lists, &file_of(&format!(r#"<diacraft:style name="Plain">{records}</diacraft:style>"#)));
    let plain = drawing(&lists, &file_of(r#"<diacraft:style name="Plain"/>"#));
    let none = drawing(&lists, &file_of(r#"<diacraft:style name="Plain"><diacraft:older list="M" level="2" width="-1cm"/></diacraft:style>"#));
    let base = read_ok(&plain).work.items;
    let yardstick = best_of_three(|| {
        read_ok(&plain);
    });
    let held_copy = held.clone();
    let (report, took) = within("the restore of 10,000 records", allowed(yardstick), move || read_ok(&held_copy));
    assert!(took <= allowed(yardstick), "took {took:?} against the yardstick's {yardstick:?}");
    assert!(report.work.items >= base + 100_000, "the tree's nodes are charged before the walk: {} against {base}", report.work.items);
    let (doc, _) = read(&held, &Limits::default()).unwrap();
    let style = doc.styles.iter().find(|s| s.ext.get().and_then(|d| d.odf.name.as_deref()) == Some("Plain")).unwrap().clone();
    let given = format!("{:?}", style.ext.get().unwrap().odf.kept.kept);
    assert_eq!(given.matches("-1cm").count(), 4_096, "the widths of the first 4,096 records are given back");
    // A record of a list style the tree does not hold: the tree is not walked again, and nothing is charged for it.
    let unwalked = read_ok(&none).work.items;
    assert!(unwalked < base + 1_000, "{unwalked} against {base}");
}
