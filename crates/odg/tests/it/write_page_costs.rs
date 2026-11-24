//! The pages of a drawing changed in bulk: hostile counts of pages, and the cost of adding, taking away and reversing them, which is held to the number of pages by
//! counting what each loop walks and never by time (`WriteWork`: edits, page steps, bytes).
//!
//! A drawing holds at most `diacraft_doc::MAX_PAGES` (1,000) pages: the reader refuses a file of more, so a save never writes one and a model of more is refused before the file is
//! read. The brief's 10,000 pages are refused here; the reversal of 5,000 pages is the planning code's own test (`write::structure::tests`), which needs no file, and a whole save is
//! reversed at the most pages a drawing may hold.

use std::sync::Arc;

use diacraft_doc::{Page, SharedVec};
use diacraft_odg::Limits;
use diacraft_odg::{OriginalPackage, WriteError, WriteOptions, WriteWork, Written, read, write_counted};

use crate::drawing::Drawing;
use crate::write_core::{Open, open};
use crate::write_page_ops::{ids, names, pages_of};

/// A drawing of `n` empty pages `p0` to `p<n-1>`.
fn many_pages(n: usize) -> Open {
    let mut d = Drawing::a4();
    for k in 0..n {
        d.pages.push_str(&format!(r#"<draw:page draw:name="p{k}" draw:master-page-name="Default"/>"#));
    }
    let bytes = d.bytes();
    let (doc, report) = read(&bytes, &Limits::default()).unwrap_or_else(|e| panic!("the drawing did not read: {} ({e})", e.class()));
    Open { bytes, doc, index: report.index }
}

/// A save, with the work it cost.
fn counted(o: &Open, limits: &Limits) -> (Result<Written, WriteError>, WriteWork) {
    let mut work = WriteWork::default();
    let options = WriteOptions { limits: limits.clone(), ..WriteOptions::default() };
    let saved = write_counted(&o.doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work);
    (saved, work)
}

/// The model with its pages in the reverse order.
fn reversed(o: &mut Open) {
    let pages: Vec<Arc<Page>> = o.doc.pages.iter().rev().cloned().collect();
    o.doc.pages = SharedVec::from_vec(pages);
}

#[test]
fn a_model_of_more_pages_than_a_drawing_may_hold_is_refused_before_the_file_is_read() {
    // 10,000 pages (the `ID` of each its own) against a file of one page, under the default cap and under a cap raised past the most the reader reads.
    let o = open(&pages_of(1));
    let mut doc = o.doc.clone();
    let template = doc.pages.get(0).unwrap().clone();
    doc.pages = SharedVec::from_vec(
        (0..10_000u32)
            .map(|id| {
                let mut p = (*template).clone();
                p.id = id;
                Arc::new(p)
            })
            .collect(),
    );
    for limits in [Limits::default(), Limits { max_pages: 1_000_000, ..Limits::default() }] {
        let mut work = WriteWork::default();
        let options = WriteOptions { limits, ..WriteOptions::default() };
        match write_counted(&doc, Some(&OriginalPackage { bytes: &o.bytes, index: &o.index }), &options, &mut work) {
            Err(e @ WriteError::Limit { what, max }) => {
                assert_eq!((e.class(), max), ("write_limit", 1_000), "{what}: a file the reader refuses is never written");
                assert!(what.contains("pages"), "{what}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(work.baseline_read.package.inflated, 0, "refused before the file was read again");
        assert_eq!((work.edits, work.page_steps), (0, 0), "and before a page was worked on");
    }
    // The most (1,000) is saved, and one page more is refused.
    let clean = |k: u32| {
        let mut p = Page::new(k, &format!("n{k}"));
        p.sheet = Arc::clone(&template.sheet);
        Arc::new(p)
    };
    let mut thousand = o.doc.clone();
    thousand.pages = SharedVec::from_vec((0..1_000u32).map(clean).collect());
    let (saved, _) = counted(&Open { bytes: o.bytes.clone(), doc: thousand.clone(), index: o.index.clone() }, &Limits::default());
    let saved = saved.unwrap_or_else(|e| panic!("1,000 pages: {} ({e})", e.class()));
    assert_eq!(read(&saved.bytes, &Limits::default()).unwrap().0.pages.len(), 1_000);
    thousand.pages.push(clean(1_000));
    let (saved, _) = counted(&Open { bytes: o.bytes.clone(), doc: thousand, index: o.index.clone() }, &Limits::default());
    assert!(matches!(saved, Err(WriteError::Limit { max: 1_000, .. })));
}

#[test]
fn pages_put_in_the_reverse_order_cost_edits_and_steps_in_proportion_to_their_number() {
    // `n` pages reversed: each page but one is moved (a longest run that keeps its order is one page), each move is two edits, and nothing walks the pages for each page.
    let run = |n: usize| {
        let mut o = many_pages(n);
        reversed(&mut o);
        let (saved, work) = counted(&o, &Limits::default());
        let saved = saved.unwrap_or_else(|e| panic!("{n} pages reversed: {} ({e})", e.class()));
        let back = read(&saved.bytes, &Limits::default()).unwrap().0;
        assert_eq!(back.pages.len(), n);
        assert_eq!(names(&back).first().map(String::as_str), Some(format!("p{}", n - 1).as_str()));
        assert_eq!(names(&back).last().map(String::as_str), Some("p0"));
        assert_eq!(ids(&back), ids(&o.doc), "the pages keep their IDs");
        let again = write_counted(
            &o.doc,
            Some(&OriginalPackage { bytes: &saved.bytes, index: &saved.index }),
            &WriteOptions::default(),
            &mut WriteWork::default(),
        )
        .unwrap();
        assert!(!again.edited, "a second save is no edit");
        (work.edits, work.page_steps, work.bytes_copied)
    };
    let (small, large) = (run(500), run(1_000));
    assert!(small.0 >= 2 * 499 && small.0 <= 3 * 500, "two edits for each page that moves: {small:?}");
    assert!(large.0 <= small.0 * 2 + 4, "edits in proportion: {small:?} {large:?}");
    assert!(large.1 <= small.1 * 2 + 4 && large.1 >= small.1 + small.1 / 2, "steps in proportion: {small:?} {large:?}");
    assert!(large.2 <= small.2 * 2 + 4096, "bytes copied in proportion: {small:?} {large:?}");
}

#[test]
fn pages_added_and_taken_away_cost_steps_in_proportion_to_their_number() {
    let run = |n: usize| {
        let mut o = many_pages(n);
        // Every other page taken away, and as many added, each with a name of its own.
        let mut pages: Vec<Arc<Page>> = o.doc.pages.iter().enumerate().filter(|(k, _)| k % 2 == 0).map(|(_, p)| p.clone()).collect();
        let first = o.doc.pages.get(0).unwrap().clone();
        for k in 0..n / 2 {
            let mut p = Page::new(u32::try_from(n + k).unwrap(), &format!("new{k}"));
            p.sheet = Arc::clone(&first.sheet);
            pages.push(Arc::new(p));
        }
        o.doc.pages = SharedVec::from_vec(pages);
        let (saved, work) = counted(&o, &Limits::default());
        let saved = saved.unwrap_or_else(|e| panic!("{n} pages: {} ({e})", e.class()));
        let back = read(&saved.bytes, &Limits::default()).unwrap().0;
        assert_eq!(back.pages.len(), o.doc.pages.len());
        (work.edits, work.page_steps, work.bytes_written)
    };
    let (small, large) = (run(400), run(800));
    assert!(large.0 <= small.0 * 2 + 4 && large.0 >= small.0, "edits in proportion: {small:?} {large:?}");
    assert!(large.1 <= small.1 * 2 + 4, "steps in proportion: {small:?} {large:?}");
    assert!(large.2 <= small.2 * 2 + 4096, "bytes written in proportion: {small:?} {large:?}");
}
