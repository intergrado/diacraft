//! The reader's side: a page's `office:annotation` (and LibreOffice's `officeooo:annotation`) elements are the document's comments, taken out of the page's
//! markup, and the ones the model cannot hold exactly stay in it. Inputs are written by hand from ODF 1.4 Part 3 (`office:annotation`, §14.1);
//! LibreOffice's own form is the one 26.8.2.2 was seen to write for a page's comment (author, initials, date, `text:p`, and `svg:x`, `svg:y`, `svg:width`, `svg:height`).

use diacraft_doc::{Comment, Document, MAX_COMMENT_TEXT_BYTES, MAX_COMMENTS};
use diacraft_odg::{Limits, read};

use crate::drawing::{Drawing, NS, odf_page};

/// The LibreOffice namespace its own comments are written in.
const OOO: &str = r#"xmlns:officeooo="http://openoffice.org/2009/office""#;

fn read_with(drawing: &Drawing) -> (Document, diacraft_odg::ReadReport) {
    drawing.read()
}

fn annotation(creator: &str, date: &str, text: &str) -> String {
    format!(
        r#"<office:annotation svg:x="5cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"><dc:creator>{creator}</dc:creator><dc:date>{date}</dc:date><meta:creator-initials>AN</meta:creator-initials><text:p>{text}</text:p></office:annotation>"#
    )
}

/// What the reader kept of page `at`'s own markup (the `.odg`'s).
fn page_kept(doc: &Document, at: usize) -> diacraft_doc::Kept {
    odf_page(doc.pages.get(at).expect("a page"))
}

fn comments(doc: &Document) -> Vec<&Comment> {
    doc.comments.iter().map(|c| &**c).collect()
}

#[test]
fn a_page_s_annotation_is_a_comment_with_its_author_date_and_lines() {
    let body = r#"<draw:rect svg:width="1cm" svg:height="1cm"/><office:annotation svg:x="5cm" svg:y="2cm" svg:width="4cm" svg:height="2cm"><dc:creator>Ada</dc:creator><dc:date>2031-03-07T20:05:09</dc:date><meta:creator-initials>AL</meta:creator-initials><text:p>first line</text:p><text:p>second line</text:p></office:annotation>"#;
    let (doc, report) = read_with(&Drawing::a4().page(body));
    let all = comments(&doc);
    assert_eq!(all.len(), 1);
    let c = all[0];
    assert_eq!((c.id, c.page, c.shape, c.parent, c.done), (1, 0, None, None, false));
    assert_eq!(&*c.text, "first line\nsecond line");
    assert_eq!(&*c.date, "2031-03-07T20:05:09");
    assert_eq!(c.edit_date, None);
    let author = doc.comments.author(c.author).expect("an author");
    assert_eq!((&*author.name, &*author.initials), ("Ada", "AL"));
    // The place the annotation has is kept beside the fields, under the names of ODF whatever the prefixes were.
    assert_eq!(c.raw.get("svg:x"), Some("5cm"));
    assert_eq!(c.raw.get("svg:y"), Some("2cm"));
    assert_eq!(c.raw.get("svg:width"), Some("4cm"));
    assert_eq!(c.raw.get("svg:height"), Some("2cm"));
    // No copy of it is kept in the page's markup.
    assert_eq!(report.kept("annotation"), 0);
    assert!(page_kept(&doc, 0).kept.is_empty(), "the page holds no markup of it");
    let (places, total) = report.index.page(0).expect("an entry").comments();
    assert_eq!((places, total), (&[0][..], 1));
}

#[test]
fn libreoffice_s_own_form_is_read_the_same() {
    let mut d = Drawing::a4();
    d.ns = format!("{NS} {OOO}");
    let body = r#"<officeooo:annotation svg:x="0.6cm" svg:y="0.3cm" svg:width="0.5cm" svg:height="0.3cm"><dc:creator>Ada Lovelace</dc:creator><meta:creator-initials>AL</meta:creator-initials><dc:date>2031-03-07T20:05:09</dc:date><text:p>First comment</text:p><text:p>second line</text:p></officeooo:annotation><officeooo:annotation svg:x="0.1cm" svg:y="0.1cm" svg:width="0.396cm" svg:height="0.496cm"><dc:creator>Bob</dc:creator><meta:creator-initials>B</meta:creator-initials><dc:date>0000-00-00T00:00:00</dc:date><text:p>Second comment</text:p></officeooo:annotation>"#;
    let (doc, report) = read_with(&d.page(body));
    let all = comments(&doc);
    assert_eq!(all.len(), 2);
    assert_eq!(&*all[0].text, "First comment\nsecond line");
    assert_eq!(&*all[1].text, "Second comment");
    assert_eq!(&*all[1].date, "0000-00-00T00:00:00");
    let names: Vec<&str> = doc.comments.authors().map(|a| &*a.name).collect();
    assert_eq!(names, ["Ada Lovelace", "Bob"]);
    assert_eq!((all[0].id, all[1].id), (1, 2), "numbered in order");
    assert_eq!(report.index.page(0).map(|p| p.comments().1), Some(2));
}

#[test]
fn white_space_is_read_as_odf_has_it() {
    // Runs collapse to one space, a leading and a trailing run is dropped, `text:s` stands for its count, `text:tab` for a tab and `text:line-break` for a line.
    let body = r#"<office:annotation><text:p>  a   b
  c<text:s text:c="2"/>d<text:tab/>e<text:line-break/>f  </text:p><text:p/><text:p><text:s text:c="3"/>x<text:s/></text:p></office:annotation>"#;
    let (doc, _) = read_with(&Drawing::a4().page(body));
    assert_eq!(&*comments(&doc)[0].text, "a b c  d\te\nf\n\n   x ");
}

#[test]
fn diacraft_s_attributes_are_read_with_their_version_and_a_comment_with_none_is_numbered_after_the_highest() {
    let body = format!(
        "{}{}{}",
        r#"<office:annotation diacraft:version="1" diacraft:id="7" diacraft:shape="3" diacraft:done="1" diacraft:edited="2031-04-01T00:00:00Z"><dc:creator>Ada</dc:creator><dc:date>d1</dc:date><text:p>root</text:p></office:annotation>"#,
        r#"<office:annotation diacraft:version="1" diacraft:id="9" diacraft:parent="7" diacraft:shape="3"><dc:creator>Bob</dc:creator><dc:date>d2</dc:date><text:p>reply</text:p></office:annotation>"#,
        r#"<office:annotation><dc:creator>Cy</dc:creator><text:p>plain</text:p></office:annotation>"#
    );
    let (doc, _) = read_with(&Drawing::a4().page(&body));
    let all = comments(&doc);
    assert_eq!(all.len(), 3);
    assert_eq!((all[0].id, all[0].shape, all[0].done, all[0].parent), (7, Some(3), true, None));
    assert_eq!(all[0].edit_date.as_deref(), Some("2031-04-01T00:00:00Z"));
    assert_eq!((all[1].id, all[1].shape, all[1].parent), (9, Some(3), Some(7)));
    assert_eq!((all[2].id, all[2].shape), (10, None), "after the highest number the file gives");
    assert_eq!(&*all[2].date, "", "a comment with no date has none");
}

#[test]
fn attributes_of_diacraft_s_without_the_version_are_another_program_s_and_are_not_read() {
    let body = r#"<office:annotation diacraft:id="7" diacraft:shape="3" diacraft:done="1"><dc:creator>Ada</dc:creator><text:p>x</text:p></office:annotation>"#;
    let (doc, _) = read_with(&Drawing::a4().page(body));
    let c = comments(&doc)[0].clone();
    assert_eq!((c.id, c.shape, c.done), (1, None, false));
}

#[test]
fn a_number_twice_goes_to_the_first_comment_and_a_reply_to_another_page_or_shape_is_a_thread_s_first() {
    let one = |id: u32, parent: Option<u32>, shape: u32| {
        let parent = parent.map(|p| format!(r#" diacraft:parent="{p}""#)).unwrap_or_default();
        format!(
            r#"<office:annotation diacraft:version="1" diacraft:id="{id}"{parent} diacraft:shape="{shape}"><dc:creator>A</dc:creator><text:p>t{id}</text:p></office:annotation>"#
        )
    };
    // Page one: 4 (shape 1), a second 4 (shape 1), a reply to 4 on shape 2 (another shape), a reply to 4 on shape 1. Page two: a reply to 4 (another page).
    let page1 = format!("{}{}{}{}", one(4, None, 1), one(4, None, 1), one(5, Some(4), 2), one(6, Some(4), 1));
    let page2 = one(8, Some(4), 1);
    let (doc, _) = read_with(&Drawing::a4().page(&page1).page(&page2));
    let all = comments(&doc);
    assert_eq!(all.iter().map(|c| c.id).collect::<Vec<_>>(), [4, 9, 5, 6, 8], "the second 4 is numbered after the highest");
    let parents: Vec<Option<u32>> = all.iter().map(|c| c.parent).collect();
    assert_eq!(parents, [None, None, None, Some(4), None]);
    assert_eq!((all[4].page, all[0].page), (1, 0));
}

#[test]
fn an_author_is_a_name_with_its_initials_and_is_made_once() {
    let body = format!(
        "{}{}{}",
        annotation("Ada", "d", "1"),
        annotation("Ada", "d", "2"),
        r#"<office:annotation><dc:creator>Ada</dc:creator><meta:creator-initials>XX</meta:creator-initials><text:p>3</text:p></office:annotation>"#
    );
    let (doc, _) = read_with(&Drawing::a4().page(&body));
    let all = comments(&doc);
    assert_eq!(all.iter().map(|c| c.author).collect::<Vec<_>>(), [1, 1, 2]);
    assert_eq!(doc.comments.authors().count(), 2);
}

#[test]
fn an_annotation_that_holds_more_than_the_model_does_stays_in_the_page_s_markup() {
    for (why, markup) in [
        (
            "a span",
            r#"<office:annotation><dc:creator>A</dc:creator><text:p>a<text:span text:style-name="T1">b</text:span></text:p></office:annotation>"#,
        ),
        ("a paragraph with a style", r#"<office:annotation><text:p text:style-name="P1">a</text:p></office:annotation>"#),
        ("a list", r#"<office:annotation><text:list><text:list-item><text:p>a</text:p></text:list-item></text:list></office:annotation>"#),
        ("an attribute of another namespace", r#"<office:annotation draw:style-name="gr1"><text:p>a</text:p></office:annotation>"#),
        ("a second creator", r#"<office:annotation><dc:creator>A</dc:creator><dc:creator>B</dc:creator></office:annotation>"#),
        ("a date string", r#"<office:annotation><meta:date-string>today</meta:date-string><text:p>a</text:p></office:annotation>"#),
        ("an XML comment", r#"<office:annotation><!-- x --><text:p>a</text:p></office:annotation>"#),
        ("text of its own", r#"<office:annotation>loose<text:p>a</text:p></office:annotation>"#),
        ("a newer version's attribute", r#"<office:annotation diacraft:version="1" diacraft:future="x"><text:p>a</text:p></office:annotation>"#),
        ("a newer version", r#"<office:annotation diacraft:version="2" diacraft:id="3"><text:p>a</text:p></office:annotation>"#),
    ] {
        let (doc, report) = read_with(&Drawing::a4().page(markup));
        assert_eq!(doc.comments.len(), 0, "{why}: not a comment");
        assert_eq!(report.kept("annotation"), 1, "{why}: kept, and counted");
        assert_eq!(report.warned("annotation_kept"), 1, "{why}: said");
        assert_eq!(page_kept(&doc, 0).kept.len(), 1, "{why}: in the page's markup");
        assert_eq!(report.index.page(0).map(|p| p.comments()), Some((&[][..], 1)), "{why}: the index says none was taken");
    }
}

#[test]
fn only_a_page_s_own_annotations_are_read_and_one_inside_a_shape_or_a_master_page_stays() {
    let inside = r#"<draw:rect svg:width="1cm" svg:height="1cm"><office:annotation><dc:creator>A</dc:creator><text:p>in a shape</text:p></office:annotation></draw:rect>"#;
    let (doc, _) = read_with(&Drawing::a4().page(inside));
    assert_eq!(doc.comments.len(), 0);
    // A master page's annotation is no page's comment.
    let mut d = Drawing::a4().page("");
    d.masters = d.masters.replace(
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"/>"#,
        r#"<style:master-page style:name="Default" style:page-layout-name="PM1"><office:annotation><dc:creator>A</dc:creator><text:p>on the master</text:p></office:annotation></style:master-page>"#,
    );
    let (doc, _) = read_with(&d);
    assert_eq!(doc.comments.len(), 0);
}

#[test]
fn an_annotation_among_unread_ones_keeps_its_place_in_the_count() {
    // The first and the third are read; the second holds a span and stays.
    let body = format!(
        "{}{}{}",
        annotation("A", "d", "one"),
        r#"<office:annotation><text:p><text:span>x</text:span></text:p></office:annotation>"#,
        annotation("A", "d", "three")
    );
    let (doc, report) = read_with(&Drawing::a4().page(&body));
    assert_eq!(comments(&doc).iter().map(|c| &*c.text).collect::<Vec<_>>(), ["one", "three"]);
    assert_eq!(report.index.page(0).map(|p| p.comments()), Some((&[0, 2][..], 3)));
}

#[test]
fn the_caps_of_the_model_leave_the_annotations_past_them_in_the_markup() {
    // A comment of more than 64 KiB is kept whole; one of exactly that is read.
    let exact = "x".repeat(MAX_COMMENT_TEXT_BYTES);
    let over = "y".repeat(MAX_COMMENT_TEXT_BYTES + 1);
    let body = format!("{}{}", annotation("A", "d", &exact), annotation("A", "d", &over));
    let (doc, report) = read_with(&Drawing::a4().page(&body));
    assert_eq!(doc.comments.len(), 1);
    assert_eq!(doc.comments.iter().next().map(|c| c.text.len()), Some(MAX_COMMENT_TEXT_BYTES));
    assert_eq!(report.kept("annotation"), 1);
    assert_eq!(report.index.page(0).map(|p| p.comments()), Some((&[0][..], 2)));
    assert_eq!(MAX_COMMENTS, 100_000);
}

#[test]
fn a_file_with_a_comment_reads_the_same_with_the_limits_of_the_reader_lowered_to_what_it_needs() {
    // The cost of a comment is charged to the read's items: one that is allowed too few items for it is refused in words, and not read as a free one.
    let (doc, report) = read_with(&Drawing::a4().page(&annotation("A", "d", "x")));
    let items = report.work.items;
    assert!(items > 0 && doc.comments.len() == 1);
    let bytes = Drawing::a4().page(&annotation("A", "d", "x")).bytes();
    let tight = Limits { max_items: (items as usize).saturating_sub(4), ..Limits::default() };
    assert!(read(&bytes, &tight).is_err(), "charged: a read allowed fewer items than it took is refused");
}

// ------------------------------------------------------------------------------------------------------ the security scan's finding: what a `text:s` stands for
//
// The binary's allocator counts (`GLOBAL`), and a count is only the read's when nothing else runs in the process, so each case runs in a child process of its own, as
// `memory_counted` does.

const CASE: &str = "DIACRAFT_ODG_COMMENTS_CASE";

/// One annotation with two paragraphs of `count` spaces for a `text:s`, `copies` times on a page; `tail` ends the second paragraph.
fn spaces(count: usize, copies: usize, tail: &str) -> Drawing {
    let one = format!(
        r#"<office:annotation><text:p><text:s text:c="{count}"/></text:p><text:p><text:s text:c="{count}"/>{tail}</text:p></office:annotation>"#
    );
    Drawing::a4().page(&one.repeat(copies))
}

/// One annotation of one paragraph of the most spaces, `copies` times.
fn one_paragraph(copies: usize) -> Drawing {
    let one = format!(r#"<office:annotation><text:p><text:s text:c="{MAX_COMMENT_TEXT_BYTES}"/></text:p></office:annotation>"#);
    Drawing::a4().page(&one.repeat(copies))
}

/// What a read of `bytes` asked the allocator for in all, what the document and its index keep, and what the read says it holds; the document too, to count it.
fn counted(bytes: &[u8]) -> (u64, i128, u64, diacraft_odg::ReadReport, Option<Document>) {
    let region = stats_alloc::Region::new(crate::GLOBAL);
    let (result, report) = diacraft_odg::read_partial(bytes, &Limits::default());
    let change = region.change();
    let kept = i128::try_from(change.bytes_allocated).unwrap_or(i128::MAX) - i128::try_from(change.bytes_deallocated).unwrap_or(i128::MAX);
    let charged = report.index.read_cost().model_bytes();
    (change.bytes_allocated as u64, kept, charged, report, result.ok())
}

#[test]
#[ignore = "run by the tests below in a process of its own"]
fn counted_child() {
    let Ok(case) = std::env::var(CASE) else { return };
    // The code a read runs, and what it makes once for the process, is made by a small read first.
    drop(counted(&one_paragraph(2).bytes()));
    match case.as_str() {
        "spaces" => {
            let hostile = spaces(MAX_COMMENT_TEXT_BYTES, 100_000, "").bytes();
            let yardstick = spaces(1, 100_000, "<text:span/>").bytes();
            let (a_h, _, _, report_h, doc_h) = counted(&hostile);
            let (comments, kept, warned) =
                (doc_h.map_or(usize::MAX, |d| d.comments.len()), report_h.kept("annotation"), report_h.warned("annotation_kept"));
            let (a_y, _, _, report_y, doc_y) = counted(&yardstick);
            println!(
                "RESULT hostile={a_h} yardstick={a_y} comments={comments} kept={kept} warned={warned} yardkept={} read={}",
                report_y.kept("annotation"),
                doc_y.is_some()
            );
        }
        "charge" => {
            let bytes = one_paragraph(400).bytes();
            let (_, kept, charged, _, doc) = counted(&bytes);
            println!("RESULT comments={} kept={kept} charged={charged}", doc.map_or(usize::MAX, |d| d.comments.len()));
        }
        "full" => {
            let (full, more) = (one_paragraph(512).bytes(), one_paragraph(612).bytes());
            let (a_full, _, _, _, doc_full) = counted(&full);
            drop(doc_full);
            let (a_more, _, _, report, doc_more) = counted(&more);
            println!(
                "RESULT full={a_full} more={a_more} comments={} kept={}",
                doc_more.map_or(usize::MAX, |d| d.comments.len()),
                report.kept("annotation")
            );
        }
        other => panic!("no case {other}"),
    }
}

/// The numbers a child printed, by name.
fn child(case: &str) -> std::collections::HashMap<String, i128> {
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "comments_read::counted_child", "--ignored", "--nocapture", "--test-threads=1"])
        .env(CASE, case)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let at = text.find("RESULT ").unwrap_or_else(|| panic!("{case}: no result in {text}"));
    let line = text[at + "RESULT ".len()..].lines().next().unwrap_or_default();
    line.split_whitespace()
        .filter_map(|w| w.split_once('='))
        .map(|(k, v)| (k.to_owned(), if v == "true" { 1 } else { v.parse().unwrap_or(-1) }))
        .collect()
}

#[test]
fn many_annotations_of_spaces_at_the_most_cost_what_plain_ones_of_the_same_size_cost() {
    // 100,000 annotations, each two paragraphs of 65,536 spaces counted by a `text:s`: 131,072 spaces a few dozen bytes stand for, past the cap of a comment (a comment is at
    // most 65,536 bytes). The yardstick is the same file with one space for each `text:s` and a span that keeps the annotation in the markup as well.
    let r = child("spaces");
    assert_eq!((r["comments"], r["kept"], r["warned"]), (0, 100_000, 100_000), "no comment: each annotation is kept as it is, and said");
    assert_eq!((r["yardkept"], r["read"]), (100_000, 1));
    // The work the file's own bytes make is what the read costs: it asked the allocator for no more than a small constant of what the yardstick did.
    assert!(r["hostile"] <= r["yardstick"] * 3, "{} bytes against the yardstick's {}", r["hostile"], r["yardstick"]);
}

#[test]
fn what_a_text_s_makes_is_charged_to_the_read_and_the_heap_is_at_or_under_the_charge() {
    // 400 annotations of exactly one paragraph of 65,536 spaces: all are comments (32 MiB of text is the cap of the document: 512 of them fit).
    let r = child("charge");
    assert_eq!(r["comments"], 400);
    assert!(r["kept"] <= r["charged"], "the heap, {} bytes, is over the charge, {}", r["kept"], r["charged"]);
}

#[test]
fn once_the_document_s_text_is_full_later_annotations_are_kept_without_being_parsed() {
    // 512 comments of 64 KiB are the 32 MiB the document holds. 100 more of the same stay in the markup, and cost the read no more than a plain annotation does.
    let r = child("full");
    assert_eq!((r["comments"], r["kept"]), (512, 100));
    // A parse of one of them is 64 KiB: a hundred of them are not.
    let extra = r["more"] - r["full"];
    assert!(extra < 100 * (MAX_COMMENT_TEXT_BYTES as i128) / 8, "the hundred cost {extra} bytes of allocation");
}

// ------------------------------------------------------------------------------------------------------ the fuzz target's recipe, run over fixed bodies

/// The recipe of the first byte 8 of `fuzz_targets/odg_read.rs`: up to 64 annotations of one page whose numbers, threads and shapes repeat and loop.
fn annotated(first: u8, body: &str) -> Drawing {
    let mut page = String::new();
    for k in 0..(1 + usize::from(first) % 64) {
        let (id, parent, shape) = ((k * 7 + usize::from(first)) % 9, (k * 5 + 1) % 9, k % 3);
        let tag = if k % 4 == 3 { "officeooo:annotation" } else { "office:annotation" };
        page.push_str(&format!(
            r#"<{tag} diacraft:version="1" diacraft:id="{id}" diacraft:parent="{parent}" diacraft:shape="{shape}" svg:x="1cm">{body}</{tag}>"#
        ));
    }
    let mut d = Drawing::a4();
    d.ns = format!("{NS} {OOO}");
    d.page(&page)
}

#[test]
fn annotations_whose_numbers_threads_and_shapes_repeat_and_loop_keep_the_models_promises() {
    let bodies = [
        "<dc:creator>Ada</dc:creator><text:p>one</text:p>",
        "<text:p>no author</text:p><text:p/>",
        "<dc:creator>A</dc:creator><dc:creator>B</dc:creator><text:p>two creators: kept in the markup</text:p>",
        "<text:p><text:span>styled</text:span></text:p>",
        "<dc:date>d</dc:date><meta:creator-initials>X</meta:creator-initials>",
        "",
        "<text:p><text:s text:c=\"3\"/>a<text:tab/>b<text:line-break/></text:p>",
        "loose text",
    ];
    for first in 0..=63_u8 {
        for body in bodies {
            let (doc, report) = annotated(first, body).read();
            let mut numbers = std::collections::HashSet::new();
            for c in doc.comments.iter() {
                assert!(numbers.insert(c.id), "a number is its own ({first}, {body})");
                if let Some(parent) = c.parent {
                    let to = doc.comments.get(parent).expect("a reply answers a comment that is there");
                    assert!(to.parent.is_none() && (to.page, to.shape) == (c.page, c.shape), "a thread is one level deep, on one page and shape");
                }
            }
            let (places, total) = report.index.page(0).expect("an entry").comments();
            assert!(places.windows(2).all(|w| w[0] < w[1]) && places.iter().all(|p| *p < total));
            assert_eq!(places.len(), doc.comments.len());
            assert_eq!(total as usize, 1 + usize::from(first) % 64, "every annotation element of the page is counted");
        }
    }
}

// ------------------------------------------------------------------------------------------------------ hostile files: counts, an annotation of megabytes, a reply cycle

#[test]
fn the_cost_of_reading_annotations_is_in_proportion_to_them() {
    // The items a read charges and the elements it walks for 50,000 annotations and for 100,000: twice, and no more (nothing looks at the comments read before for each new one).
    let body = |n: usize| {
        let one = annotation("Ada", "2031-03-07T20:05:09Z", "a line of text");
        Drawing::a4().page(&one.repeat(n))
    };
    let (_, half) = body(50_000).read();
    let (doc, whole) = body(100_000).read();
    assert_eq!(doc.comments.len(), 100_000);
    let ratio = |a: u64, b: u64| a as f64 / b as f64;
    assert!(ratio(whole.work.items, half.work.items) < 2.05, "{} items against {}", whole.work.items, half.work.items);
    assert!(ratio(whole.work.elements, half.work.elements) < 2.05, "{} elements against {}", whole.work.elements, half.work.elements);
    assert!(ratio(whole.work.attributes, half.work.attributes) < 2.05);
}

#[test]
fn an_annotation_of_megabytes_stays_in_the_markup_at_the_cost_of_its_bytes() {
    let big = "x".repeat(4 * 1024 * 1024);
    let (doc, report) = Drawing::a4().page(&annotation("Ada", "d", &big)).read();
    assert_eq!(doc.comments.len(), 0);
    assert_eq!(report.kept("annotation"), 1);
    // Charged as the markup it is kept as: the bytes of its text at a share of an item for each 64, with a small constant of items beside.
    assert!(report.work.items < (4 * 1024 * 1024 / 64) * 4, "{} items", report.work.items);
}

#[test]
fn a_reply_cycle_a_reply_to_itself_and_a_chain_are_one_level_deep() {
    let one = |id: u32, parent: u32| {
        format!(
            r#"<office:annotation diacraft:version="1" diacraft:id="{id}" diacraft:parent="{parent}"><dc:creator>A</dc:creator><text:p>t{id}</text:p></office:annotation>"#
        )
    };
    // 1 and 2 answer each other; 3 answers itself; 4 answers 5, which answers 6, which answers 4 (a ring of three); 7 answers 8, 8 answers 9 and 9 answers nobody (a chain).
    let body = [one(1, 2), one(2, 1), one(3, 3), one(4, 5), one(5, 6), one(6, 4), one(7, 8), one(8, 9), one(9, 99)].concat();
    let (doc, _) = Drawing::a4().page(&body).read();
    assert_eq!(doc.comments.len(), 9);
    for c in doc.comments.iter() {
        if let Some(parent) = c.parent {
            let to = doc.comments.get(parent).expect("a comment that is there");
            assert!(to.parent.is_none(), "{} answers {} which answers {:?}", c.id, to.id, to.parent);
        }
    }
    assert_eq!(doc.comments.get(3).unwrap().parent, None);
    assert_eq!(doc.comments.get(7).unwrap().parent, Some(9), "a chain that ends is moved to the first of the thread");
}
