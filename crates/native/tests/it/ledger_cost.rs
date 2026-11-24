//! Item 2 (the report, concern 4): **what a hostile identifier ledger costs a read.** A `state.xml` of 200,001 rows opened in 186 ms against 2.9 ms for a clean one,
//! 63 times, and nothing said whether that was the ledger's own cost or the cost of any part of its size. It is the size's: **the cost is linear in the bytes of the part, and bounded by the
//! caps of the read**, which these tests count and (`--ignored`) time beside a yardstick of plain pages of the same size.
//!
//! * The part is a tree (the spec's 6.1): parsed at the items of one tree (2 million, three for a row), its rows walked once, and let go. **A row is kept only for a page the drawing
//!   has** (2.7), so the ledger the read holds is at most one mark for each page (1,000), whatever the rows of the file.
//! * The cost of a row is its bytes: one event of the parser and a few steps of the walk. A file of the most rows a tree may have is 27 MB of text and some 0.6 seconds, and it deflates to a
//!   few kilobytes (the container's ratio of 1,000 is the same for any XML part: a hostile page part of that size costs more per byte than the ledger does).

use diacraft_native::read;

use crate::util::*;
use crate::zipbuild::Item;

/// A package of `pages` (`(name, text)`, the first is page 1) and the state part `state`.
fn package_with_state(pages: &[(String, Vec<u8>)], state: &[u8]) -> Vec<u8> {
    let page_refs: String =
        pages.iter().enumerate().map(|(i, (name, _))| format!("<diacraft:page-ref id=\"{}\" part=\"{name}\"/>\n", i + 1)).collect();
    let document = part(&format!("<diacraft:document xmlns:diacraft=\"{NS}\" title=\"Untitled\">\n{STYLE_BLOCK}{page_refs}</diacraft:document>"));
    let mut rows: Vec<(&str, &str, &str)> = vec![("document.xml", "document", "")];
    rows.extend(pages.iter().map(|(n, _)| (n.as_str(), "page", "")));
    rows.push(("state.xml", "state", ""));
    let mut items = vec![
        Item::stored("mimetype", MEDIA_DRAWING.as_bytes()),
        Item::deflated("manifest.xml", &manifest("1.0", "drawing", &rows)),
        Item::deflated("document.xml", &document),
    ];
    items.extend(pages.iter().map(|(n, t)| Item::deflated(n, t)));
    items.push(Item::deflated("state.xml", state));
    package(&items)
}

/// The one page the drawings of these tests have: page 1, with a shape of `id` 3.
fn one_page() -> Vec<(String, Vec<u8>)> {
    vec![(
        "pages/page1.xml".to_owned(),
        part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"1\" name=\"P1\"><diacraft:shape id=\"3\"/></diacraft:page>")),
    )]
}

/// A `state.xml` of `rows` ledger rows, none the copy of another: half name the page the drawing has (with a mark of every size) and half name pages it does not have.
fn hostile_state(rows: usize) -> Vec<u8> {
    let mut text = format!("<diacraft:state xmlns:diacraft=\"{NS}\">");
    for k in 0..rows {
        let page = if k % 2 == 0 { 1 } else { 2 + k % 5_000 };
        text.push_str(&format!("<diacraft:ledger page=\"{page}\" mark=\"{}\"/>", 1 + k % 4_000_000_000));
    }
    text.push_str("</diacraft:state>");
    part(&text)
}

/// **Counted: the cost is linear in the rows.** The events of the parser are one for each row and a few for the rest of the package, the bytes inflated are the part's, and the ledger the
/// read keeps is one mark, for the one page the drawing has, however many rows the file has: at four sizes, each double the one before.
#[test]
fn a_hostile_ledger_costs_a_read_in_proportion_to_its_rows_and_keeps_one_mark_for_a_page_the_drawing_has() {
    let mut seen: Vec<(u64, u64)> = Vec::new();
    for rows in [10_000usize, 20_000, 40_000, 80_000] {
        let state = hostile_state(rows);
        let (_, opened) = open_ok(&package_with_state(&one_page(), &state));
        let work = opened.work;
        assert!(
            work.inflated >= state.len() as u64 && work.inflated < state.len() as u64 + 20_000,
            "{rows} rows: {} bytes inflated for a part of {}",
            work.inflated,
            state.len()
        );
        // What is kept: a mark for the page the drawing has (the largest of the rows for it, and never below its highest shape `id`), and none for the others.
        let marks: Vec<(u32, u32)> = opened.state.ledger.0.iter().map(|(p, m)| (*p, *m)).collect();
        assert_eq!(marks.len(), 1, "{rows} rows: {marks:?}");
        assert_eq!(marks[0].0, 1);
        seen.push((rows as u64, work.xml_events));
    }
    // Every row costs the parser the same number of events (an empty element is one or two), and the rest of the package costs a few: the events are the rows' and a constant.
    let per_row = (seen[1].1 - seen[0].1) / (seen[1].0 - seen[0].0);
    assert!((1..=2).contains(&per_row), "{seen:?}");
    for pair in seen.windows(2) {
        assert_eq!(pair[1].1 - pair[0].1, per_row * (pair[1].0 - pair[0].0), "double the rows, double the events: {pair:?}");
    }
    assert!(seen[0].1 < per_row * seen[0].0 + 200, "the rest of the package is a few events: {seen:?}");
}

/// **Counted: the cost is bounded by the caps of the read.** The part is a tree, and a tree holds 2 million items (the root and its declaration are two, a row is three: its element
/// and its two attributes), so a ledger has 666,666 rows at the most: that many are read, and one more is refused in words that say a tree is too complex, before its rows are walked.
#[test]
fn a_ledger_is_held_to_the_items_of_a_tree_the_most_rows_it_may_have_are_read_and_one_more_is_refused() {
    let most = (2_000_000 - 2) / 3;
    let (_, opened) = open_ok(&package_with_state(&one_page(), &hostile_state(most)));
    assert!(opened.work.xml_events >= most as u64 && opened.work.xml_events <= 2 * most as u64 + 200, "{}", opened.work.xml_events);
    assert_eq!(opened.state.ledger.0.len(), 1);
    let error = refused(&package_with_state(&one_page(), &hostile_state(most + 1)));
    assert_eq!(error.class(), "xml_too_complex");
    assert!(error.to_string().contains("state.xml"), "{error}");
}

// ----------------------------------------------------------------------------------------------------------------------------- the yardstick

/// The time `read` takes of `bytes`, the least of three.
fn time_of(bytes: &[u8]) -> std::time::Duration {
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            let _ = read(bytes, &diacraft_native::Limits::default());
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// A package of plain pages of about `size` inflated bytes, each shape of its own (nothing is shared): the yardstick of a crafted part of that size.
fn plain_of(size: usize) -> Vec<u8> {
    let page = |i: usize| {
        let shapes: String = (0..300)
            .map(|k| {
                let v = i * 1_000 + k;
                format!(
                    "<diacraft:shape id=\"{k}\" name=\"S{k}\"><diacraft:sheet><diacraft:Cell N=\"PinX\" V=\"{v}\"/></diacraft:sheet></diacraft:shape>"
                )
            })
            .collect();
        part(&format!("<diacraft:page xmlns:diacraft=\"{NS}\" id=\"{i}\" name=\"P{i}\">{shapes}</diacraft:page>"))
    };
    let each = page(1).len();
    let n = (size / each).clamp(1, 900);
    let pages: Vec<(String, Vec<u8>)> = (1..=n).map(|i| (format!("pages/page{i}.xml"), page(i))).collect();
    package_with_state(&pages, &state_empty())
}

/// **Timed, at four sizes, beside plain pages of the same size** (`--ignored`, alone, on a quiet machine): the ledger costs no more per byte than plain content does, and the cost is
/// linear in the size. Measured on beast (2026-11-21, load 12 to 30): see the report.
#[test]
#[ignore = "timed: run beside its yardstick on a quiet machine"]
fn timed_a_hostile_ledger_against_plain_pages_of_the_same_size_at_four_sizes() {
    let mut ledger_times = Vec::new();
    for rows in [25_000usize, 50_000, 100_000, 200_001] {
        let state = hostile_state(rows);
        let crafted = package_with_state(&one_page(), &state);
        let (case, yardstick) = (time_of(&crafted), time_of(&plain_of(state.len())));
        println!("{rows} rows, {} KB of state.xml: ledger {case:?}, plain pages {yardstick:?}", state.len() / 1_000);
        assert!(case <= yardstick * 2, "the ledger of {rows} rows costs {case:?}, the plain pages of its size {yardstick:?}");
        ledger_times.push(case.as_secs_f64());
    }
    // Linear: eight times the rows is not more than twelve times the time (a constant of the rest of the package, and noise, beside it).
    let (first, last) = (ledger_times[0], ledger_times[3]);
    println!("the ledger: {:.3} s at 25,000 rows, {:.3} s at 200,001: {:.1} times for 8 times the rows", first, last, last / first);
    assert!(last <= first * 12.0, "{first} against {last}");
}
