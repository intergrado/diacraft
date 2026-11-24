//! The comments part of a VSDX package: read into the model, written by the splice writer, the model writer and for a new drawing, and
//! the hostile parts the reader leaves as the bytes they are.

use std::sync::Arc;

use diacraft_doc::{Comment, CommentAuthor, Comments, Document, NewComment, PartRole, RawAttrs};
use diacraft_testkit::package::{NS_2012, NS_R, PageSpec, Pkg, VIS, VsdxBuilder};
use diacraft_vsdx::{OriginalBytes, WriteOptions, read, work, write_with};

use crate::read_ok;

const DOC: &str = "/visio/document.xml";
const CT: &str = "application/vnd.ms-visio.comments+xml"; // brand-ok: a content type the format defines

fn comments_xml(body: &str) -> String {
    format!(r#"<Comments xmlns="{NS_2012}" xmlns:r="{NS_R}" ShowCommentTags="0">{body}</Comments>"#)
}

const TWO: &str = r#"<AuthorList><AuthorEntry ID="1" Name="Ada Lovelace" Initials="AL" ResolutionID="r1" SMTP="ada@example.org"/><AuthorEntry ID="2" Name="Bob" Initials="B"/></AuthorList><CommentList><CommentEntry AuthorID="1" PageID="1" ShapeID="7" Date="2031-03-07T20:05:09Z" CommentID="1">Check this &amp; that
second line</CommentEntry><CommentEntry AuthorID="2" PageID="1" Date="2031-03-08T10:00:00Z" EditDate="2031-03-09T10:00:00Z" CommentID="3" Done="1" AutoCommentType="2">On the page</CommentEntry></CommentList>"#;

fn package(comments: Option<&str>) -> Vec<u8> {
    let mut extra = Pkg::new();
    if let Some(xml) = comments {
        extra = extra.part("/visio/comments.xml", CT, xml).rel(DOC, "rId91", &format!("{VIS}comments"), "comments.xml");
    }
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build()
}

fn part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    pkg.part(name).map(<[u8]>::to_vec)
}

fn save(doc: &Document, original: &[u8], baseline: &Document) -> Vec<u8> {
    write_with(doc, Some(&OriginalBytes { bytes: original, baseline: Some(baseline) }), &WriteOptions::default())
        .unwrap_or_else(|e| panic!("{e}"))
        .bytes
}

fn new_comment(doc: &Document, text: &str) -> NewComment {
    let author = doc.comments.authors().next().map_or(1, |a| a.id);
    NewComment { parent: None, page: 1, shape: Some(1), author, text: Arc::from(text), date: Arc::from("2031-04-01T00:00:00Z") }
}

#[test]
fn the_authors_and_comments_are_read_and_the_part_is_the_models_not_a_held_part() {
    let (doc, report) = read_ok(&package(Some(&comments_xml(TWO))));
    let c = &doc.comments;
    assert_eq!(c.show_tags, Some(false));
    assert_eq!(c.authors().count(), 2);
    let ada = c.author(1).unwrap();
    assert_eq!((&*ada.name, &*ada.initials), ("Ada Lovelace", "AL"));
    assert_eq!(ada.raw.get("SMTP"), Some("ada@example.org"));
    assert_eq!(ada.raw.get("ResolutionID"), Some("r1"));
    let first = c.get(1).unwrap();
    assert_eq!((first.page, first.shape, first.author), (1, Some(7), 1));
    assert_eq!(&*first.text, "Check this & that\nsecond line");
    let page_comment = c.get(3).unwrap();
    assert_eq!(page_comment.shape, None, "no ShapeID is a comment on the page");
    assert!(page_comment.done);
    assert_eq!(page_comment.edit_date.as_deref(), Some("2031-03-09T10:00:00Z"));
    assert_eq!(page_comment.raw.get("AutoCommentType"), Some("2"));
    assert!(doc.parts.iter().all(|p| &*p.name != "/visio/comments.xml"), "the part is the model's now");
    assert_eq!(report.warning_count("comments_unreadable"), 0);
}

#[test]
fn a_comments_part_in_another_namespace_stays_a_part_held_as_bytes() {
    let (doc, report) = read_ok(&package(Some(r#"<Comments xmlns="urn:someone-else"><x/></Comments>"#)));
    assert!(doc.comments.is_empty());
    let held = doc.parts.iter().find(|p| &*p.name == "/visio/comments.xml").unwrap();
    assert_eq!(held.role, PartRole::Other);
    assert_eq!(
        report.warning_count("comments_unexpected_root") + report.warning_count("comments_unreadable"),
        0,
        "another writer's part is kept with no word"
    );
}

#[test]
fn a_save_that_changes_no_comment_keeps_the_part_byte_for_byte() {
    let original = package(Some(&comments_xml(TWO)));
    let (doc, _) = read_ok(&original);
    let saved = save(&doc, &original, &doc);
    assert_eq!(part(&saved, "/visio/comments.xml"), part(&original, "/visio/comments.xml"));
}

#[test]
fn a_comment_added_is_written_into_the_files_part_and_read_back() {
    let original = package(Some(&comments_xml(TWO)));
    let (base, _) = read_ok(&original);
    let mut doc = base.clone();
    let id = doc.comments.add(new_comment(&doc, "added <in> DiaCraft & saved")).unwrap();
    let saved = save(&doc, &original, &base);
    let (back, _) = read_ok(&saved);
    assert_eq!(back.comments.len(), 3);
    assert_eq!(&*back.comments.get(id).unwrap().text, "added <in> DiaCraft & saved");
    assert_eq!(back.comments.show_tags, Some(false));
    // What was there is there: its author's other attributes, its done mark, its lines.
    assert_eq!(back.comments.author(1).unwrap().raw.get("SMTP"), Some("ada@example.org"));
    assert!(back.comments.get(3).unwrap().done);
    assert_eq!(&*back.comments.get(1).unwrap().text, "Check this & that\nsecond line");
    // The rest of the package is the original's.
    assert_eq!(part(&saved, "/visio/pages/page1.xml"), part(&original, "/visio/pages/page1.xml"));
}

#[test]
fn a_file_with_no_comments_part_gets_one_when_a_comment_is_added() {
    let original = package(None);
    let (base, _) = read_ok(&original);
    let mut doc = base.clone();
    let author = doc.comments.author_for("Ada", "A").unwrap();
    let mut n = new_comment(&doc, "the first");
    n.author = author;
    doc.comments.add(n).unwrap();
    let saved = save(&doc, &original, &base);
    assert!(part(&saved, "/visio/comments.xml").is_some());
    let (back, _) = read_ok(&saved);
    assert_eq!(back.comments.len(), 1);
    assert_eq!(&*back.comments.get(1).unwrap().text, "the first");
    // The content type and the relationship are the format's.
    let pkg = diacraft_opc::Package::read(&saved, &diacraft_opc::Limits::default()).unwrap();
    assert_eq!(pkg.content_type("/visio/comments.xml"), Some(CT));
}

#[test]
fn all_comments_taken_away_leave_a_part_with_empty_lists_and_the_file_reads() {
    let original = package(Some(&comments_xml(TWO)));
    let (base, _) = read_ok(&original);
    let mut doc = base.clone();
    doc.comments.remove(1).unwrap();
    doc.comments.remove(3).unwrap();
    let saved = save(&doc, &original, &base);
    let (back, _) = read_ok(&saved);
    assert_eq!(back.comments.len(), 0);
}

#[test]
fn a_model_written_whole_writes_its_comments_too() {
    let original = package(Some(&comments_xml(TWO)));
    let (doc, _) = read_ok(&original);
    // No file to splice into: the model writer.
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.comments, doc.comments);
    // And a comment added to it.
    let mut edited = doc.clone();
    edited.comments.add(new_comment(&doc, "one more")).unwrap();
    let written = write_with(&edited, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.comments.len(), 3);
}

#[test]
fn a_new_drawing_with_comments_is_written_with_its_part() {
    let mut doc = Document::new_blank();
    let author = doc.comments.author_for("Ada", "A").unwrap();
    doc.comments
        .add(NewComment { parent: None, page: 1, shape: None, author, text: Arc::from("on a new drawing"), date: Arc::from("2031-04-01T00:00:00Z") })
        .unwrap();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, _) = read_ok(&written.bytes);
    assert_eq!(back.comments.len(), 1);
    assert_eq!(&*back.comments.get(1).unwrap().text, "on a new drawing");
}

/// A model of one page with a comment on shape 4 and a reply to it.
fn thread() -> Document {
    let mut doc = Document::new_blank();
    let author = doc.comments.author_for("Ada", "A").unwrap();
    let root = doc
        .comments
        .add(NewComment { parent: None, page: 1, shape: Some(4), author, text: Arc::from("q"), date: Arc::from("2031-04-01T00:00:00Z") })
        .unwrap();
    doc.comments
        .add(NewComment { parent: Some(root), page: 0, shape: None, author, text: Arc::from("a"), date: Arc::from("2031-04-01T00:01:00Z") })
        .unwrap();
    doc
}

#[test]
fn a_reply_is_written_as_a_comment_of_the_same_place_and_its_thread_rides_in_diacraft_s_part() {
    let doc = thread();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.comments.len(), 2);
    let reply = back.comments.get(2).unwrap();
    assert_eq!((reply.page, reply.shape), (1, Some(4)), "it sits where its comment sits");
    assert_eq!(reply.parent, Some(1), "DiaCraft's part says whose answer it is, so a DiaCraft read has the thread");
    assert_eq!(back.comments.get(1).unwrap().parent, None);
    assert_eq!(report.carry.kept, 1, "{:?}", report.carry);
    // Another program reads the comments part, which holds two comments and no thread.
    let part = part(&written.bytes, "/visio/comments.xml").map(|b| String::from_utf8(b).unwrap()).unwrap();
    assert_eq!(part.matches("<CommentEntry ").count(), 2);
    assert!(!part.contains("parent"), "{part}");
}

#[test]
fn the_splice_writer_carries_the_thread_too_and_a_file_with_no_reply_carries_no_item() {
    let original = package(Some(&comments_xml(TWO)));
    let (doc, _) = read_ok(&original);
    let plain = save(&doc, &original, &doc);
    let sol = |bytes: &[u8]| part(bytes, "/visio/solutions/solution1.xml").map(|b| String::from_utf8(b).unwrap()).unwrap_or_default();
    assert!(!sol(&plain).contains("kind=\"comments\""), "no reply, no item");
    let mut edited = doc.clone();
    edited
        .comments
        .add(NewComment { parent: Some(1), page: 1, shape: Some(7), author: 2, text: Arc::from("a reply"), date: Arc::from("2031-04-01T00:00:00Z") })
        .unwrap();
    let saved = save(&edited, &original, &doc);
    let (back, _) = read_ok(&saved);
    assert_eq!(back.comments.len(), 3);
    assert_eq!(back.comments.get(4).map(|c| c.parent), Some(Some(1)));
    // A second save of what was read is no change to the part.
    let again = save(&back, &saved, &back);
    assert_eq!(part(&again, "/visio/comments.xml"), part(&saved, "/visio/comments.xml"));
    assert_eq!(read_ok(&again).0.comments.get(4).map(|c| c.parent), Some(Some(1)));
}

fn items_of(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let pkg = diacraft_opc::Package::read(bytes, &diacraft_opc::Limits::default()).unwrap();
    let mut out = vec![("[Content_Types].xml".to_owned(), pkg.content_types_bytes().to_vec())];
    out.extend(pkg.parts().map(|p| (p.trim_start_matches('/').to_owned(), pkg.part(p).unwrap().to_vec())));
    out
}

/// The package with the part `name` changed by `f`, which must change it.
fn edit_part(bytes: &[u8], name: &str, f: impl FnOnce(&str) -> String) -> Vec<u8> {
    let mut items = items_of(bytes);
    let slot = items.iter_mut().find(|(n, _)| format!("/{n}") == name).unwrap_or_else(|| panic!("no part {name}"));
    let before = String::from_utf8(slot.1.clone()).unwrap();
    let after = f(&before);
    assert_ne!(before, after, "the edit of {name} changed nothing");
    slot.1 = after.into_bytes();
    diacraft_testkit::package::zip(&items.iter().map(|(n, b)| diacraft_testkit::package::ZipItem::deflated(n, b)).collect::<Vec<_>>())
}

const SOLUTION: &str = "/visio/solutions/solution1.xml";

#[test]
fn the_thread_is_dropped_when_another_program_changed_the_comments_part() {
    let written = write_with(&thread(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let edited = edit_part(&written.bytes, "/visio/comments.xml", |text| text.replacen(">a</CommentEntry>", ">changed</CommentEntry>", 1));
    let (back, report) = read_ok(&edited);
    assert_eq!(back.comments.len(), 2);
    assert_eq!(back.comments.get(2).unwrap().parent, None, "the comments are another program's now: no thread is put on them");
    assert_eq!(report.carry.stale, 1, "{:?}", report.carry);
}

#[test]
fn links_that_cannot_be_a_thread_are_left_out_and_counted() {
    let written = write_with(&thread(), None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    let solution = String::from_utf8(part(&written.bytes, SOLUTION).unwrap()).unwrap();
    assert!(solution.contains("<link id=\"2\" parent=\"1\"/>"), "{solution}");
    for (what, link) in [
        ("a comment the file lacks", "<link id=\"2\" parent=\"9\"/>"),
        ("a comment its own answer", "<link id=\"2\" parent=\"2\"/>"),
        ("a reply to a reply", "<link id=\"2\" parent=\"1\"/><link id=\"1\" parent=\"2\"/>"),
    ] {
        let edited = edit_part(&written.bytes, SOLUTION, |text| text.replacen("<link id=\"2\" parent=\"1\"/>", link, 1));
        let (back, report) = read_ok(&edited);
        let threads = back.comments.iter().filter(|c| c.parent.is_some()).count();
        assert!(threads <= 1 && back.comments.len() == 2, "{what}: {threads}");
        if what != "a reply to a reply" {
            assert_eq!(threads, 0, "{what}");
            assert_eq!(report.carry.left_out, 1, "{what}: {:?}", report.carry);
        }
    }
    // A link that is no number, or more links than there are comments, is a damaged item.
    for (what, link) in [
        ("no number", "<link id=\"x\" parent=\"1\"/>"),
        ("too many", "<link id=\"2\" parent=\"1\"/><link id=\"2\" parent=\"1\"/><link id=\"2\" parent=\"1\"/>"),
    ] {
        let edited = edit_part(&written.bytes, SOLUTION, |text| text.replacen("<link id=\"2\" parent=\"1\"/>", link, 1));
        let (back, report) = read_ok(&edited);
        assert_eq!(back.comments.iter().filter(|c| c.parent.is_some()).count(), 0, "{what}");
        assert!(report.warning_count("diacraft_item_damaged") >= 1, "{what}: {:?}", report.warning_counts);
    }
}

#[test]
fn twenty_thousand_comments_with_a_reply_for_each_second_one_keep_their_threads_and_cost_a_step_for_each() {
    // The comments part is a small part of the file (4 MiB), and 20,000 comments are about 2 MB of it: more are held as the bytes they are, and no thread is put on bytes.
    let author = CommentAuthor { id: 1, name: Arc::from("Ada"), initials: Arc::from("A"), raw: RawAttrs::new() };
    let list: Vec<Comment> = (1..=20_000_u32)
        .map(|id| Comment {
            id,
            parent: (id % 2 == 0).then(|| id - 1),
            page: 1,
            shape: Some(4),
            author: 1,
            text: Arc::from("x"),
            date: Arc::from("2031-04-01T00:00:00Z"),
            edit_date: None,
            done: false,
            raw: RawAttrs::new(),
        })
        .collect();
    let mut doc = Document::new_blank();
    doc.comments = Comments::from_parts(None, RawAttrs::new(), vec![author], list).unwrap();
    let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    work::reset();
    let (back, report) = read_ok(&written.bytes);
    assert_eq!(back.comments.iter().filter(|c| c.parent.is_some()).count(), 10_000, "{:?} {:?}", report.carry, report.warning_counts);
    assert_eq!(report.carry.left_out, 0, "{:?}", report.carry);
    assert_eq!(work::carry_items_judged(), 1, "one item is judged, whatever the number of comments");
    assert!(work::carry_items_stamped() < 20_100, "and the comments are stamped once: {} steps", work::carry_items_stamped());
}

// ------------------------------------------------------------------------------------------------------------------------------- hostile

fn held_as_bytes(xml: &str) -> (Document, diacraft_vsdx::ReadReport) {
    let (doc, report) = read_ok(&package(Some(xml)));
    assert!(doc.comments.is_empty());
    assert!(doc.parts.iter().any(|p| &*p.name == "/visio/comments.xml"), "the part stays as the bytes it is");
    (doc, report)
}

#[test]
fn a_part_the_model_cannot_hold_exactly_is_kept_as_bytes_and_the_read_says_so() {
    for (what, body) in [
        (
            "an id used twice",
            r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="3">a</CommentEntry><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="3">b</CommentEntry></CommentList>"#,
        ),
        (
            "a comment with an element inside",
            r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="3">a<b/></CommentEntry></CommentList>"#,
        ),
        (
            "a number that is not a number",
            r#"<CommentList><CommentEntry AuthorID="1" PageID="x" Date="d" CommentID="3">a</CommentEntry></CommentList>"#,
        ),
        ("no CommentID", r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d">a</CommentEntry></CommentList>"#),
        ("no Date", r#"<CommentList><CommentEntry AuthorID="1" PageID="1" CommentID="3">a</CommentEntry></CommentList>"#),
        ("a repeated author", r#"<AuthorList><AuthorEntry ID="1"/><AuthorEntry ID="1"/></AuthorList>"#),
    ] {
        let (_, report) = held_as_bytes(&comments_xml(body));
        assert_eq!(report.warning_count("comments_unreadable"), 1, "{what}");
    }
}

#[test]
fn a_comment_of_more_than_the_cap_of_text_is_kept_as_bytes() {
    let long = "x".repeat(diacraft_doc::MAX_COMMENT_TEXT_BYTES + 1);
    let body = format!(r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="1">{long}</CommentEntry></CommentList>"#);
    let (_, report) = held_as_bytes(&comments_xml(&body));
    assert_eq!(report.warning_count("comments_unreadable"), 1);
}

#[test]
fn a_part_of_megabytes_is_not_parsed_into_comments() {
    // Past the size of a small part: it stays as it is, and the read costs a warning and not a walk.
    let entry = r#"<CommentEntry AuthorID="1" PageID="1" Date="2031-03-07T20:05:09Z" CommentID="N">some words of a comment</CommentEntry>"#;
    let mut body = String::from(r#"<CommentList>"#);
    let mut n = 0_u32;
    while body.len() < 5 * 1024 * 1024 {
        n += 1;
        body.push_str(&entry.replace('N', &n.to_string()));
    }
    body.push_str("</CommentList>");
    work::reset();
    let (_, report) = held_as_bytes(&comments_xml(&body));
    assert_eq!(report.warning_count("comments_unreadable"), 1);
    assert_eq!(work::comment_entries_read(), 0, "no entry was looked at");
}

#[test]
fn the_cost_of_reading_comments_is_one_look_at_each_entry() {
    // The yardstick: 20,000 comments and 2 authors cost 20,002 looks, whatever else is in the part, and the model has them all.
    let entry = r#"<CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="N">c</CommentEntry>"#;
    let mut list = String::from(
        "<AuthorList><AuthorEntry ID=\"1\" Name=\"A\" Initials=\"A\"/><AuthorEntry ID=\"2\" Name=\"B\" Initials=\"B\"/></AuthorList><CommentList>",
    );
    for n in 1..=20_000_u32 {
        list.push_str(&entry.replace('N', &n.to_string()));
    }
    list.push_str("</CommentList>");
    work::reset();
    let (doc, _) = read_ok(&package(Some(&comments_xml(&list))));
    assert_eq!(doc.comments.len(), 20_000);
    assert_eq!(work::comment_entries_read(), 20_002);
}

#[test]
fn replies_a_file_cannot_hold_do_not_exist_and_a_chain_of_parents_is_not_a_part_of_the_format() {
    // A comments part has no parent at all: an attribute that names one is kept as the unknown attribute it is.
    let body = r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="1" ParentID="2">a</CommentEntry><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="2" ParentID="1">b</CommentEntry></CommentList>"#;
    let (doc, _) = read_ok(&package(Some(&comments_xml(body))));
    assert!(doc.comments.iter().all(|c| c.parent.is_none()));
    assert_eq!(doc.comments.get(1).unwrap().raw.get("ParentID"), Some("2"));
}

#[test]
fn from_parts_keeps_what_a_reader_gives_it() {
    // The model type the reader builds: a smoke test that the public constructor and the part agree on the shape of an entry.
    let a = CommentAuthor { id: 1, name: Arc::from("A"), initials: Arc::from("A"), raw: RawAttrs::new() };
    let c = Comment {
        id: 1,
        parent: None,
        page: 1,
        shape: None,
        author: 1,
        text: Arc::from("t"),
        date: Arc::from("d"),
        edit_date: None,
        done: false,
        raw: RawAttrs::new(),
    };
    let comments = Comments::from_parts(None, RawAttrs::new(), vec![a], vec![c]).unwrap();
    let xml = diacraft_vsdx::comments::root(&comments);
    assert_eq!(xml.name, "Comments");
    assert!(read(&package(None), &diacraft_vsdx::Limits::default()).is_ok());
}

// ------------------------------------------------------------------------------------------------------- the part a save writes (security scan)

/// A package whose relationship of the type `comments` leads to `target` (a part that is not a comments part).
fn package_pointing_at(target: &str) -> Vec<u8> {
    let extra = Pkg::new().rel(DOC, "rId91", &format!("{VIS}comments"), target);
    VsdxBuilder::new().page(PageSpec::new(1, "P")).with(extra).build()
}

#[test]
fn a_comments_relationship_that_leads_to_another_part_does_not_make_the_save_overwrite_it() {
    for target in ["pages/page1.xml", "document.xml", "pages/pages.xml"] {
        let original = package_pointing_at(target);
        let (base, _) = read_ok(&original);
        assert!(base.comments.is_empty(), "{target}: no comments were read from it");
        let mut doc = base.clone();
        let author = doc.comments.author_for("Ada", "A").unwrap();
        let mut n = new_comment(&doc, "a comment");
        n.author = author;
        doc.comments.add(n).unwrap();
        // The splice writer.
        let saved = save(&doc, &original, &base);
        for part in ["/visio/pages/page1.xml", "/visio/document.xml", "/visio/pages/pages.xml"] {
            assert_eq!(part_of(&saved, part), part_of(&original, part), "{target}: {part} is the original's");
        }
        let (back, _) = read_ok(&saved);
        assert_eq!(back.comments.len(), 1, "{target}: the comment is in a part of its own");
        // The model writer.
        let written = write_with(&doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{target}: {e}"));
        let (back, _) = read_ok(&written.bytes);
        assert_eq!(back.comments.len(), 1, "{target}: written whole, the comment is in a part of its own");
        assert_eq!(back.pages.len(), 1, "{target}: the page is still a page");
    }
}

fn part_of(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    part(bytes, name)
}

#[test]
fn the_part_a_read_took_is_the_one_a_save_rewrites() {
    let original = package(Some(&comments_xml(TWO)));
    let (base, _) = read_ok(&original);
    assert_eq!(base.comments.part.as_deref(), Some("/visio/comments.xml"));
    let mut doc = base.clone();
    doc.comments.add(new_comment(&doc, "one more")).unwrap();
    let saved = save(&doc, &original, &base);
    let pkg = diacraft_opc::Package::read(&saved, &diacraft_opc::Limits::default()).unwrap();
    assert_eq!(pkg.parts().filter(|p| p.to_ascii_lowercase().contains("comments")).count(), 1, "one comments part");
}

// ------------------------------------------------------------------------------------------------------- the attributes kept (security scan)

#[test]
fn the_unknown_attributes_of_the_comments_are_charged_by_the_read() {
    let attrs = |n: usize, size: usize| (0..n).map(|i| format!(r#" Extra{i}="{}""#, "v".repeat(size))).collect::<String>();
    let comment_with = |extra: &str| {
        let entries: String =
            (1..=20).map(|n| format!(r#"<CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="{n}"{extra}>c</CommentEntry>"#)).collect();
        comments_xml(&format!("<CommentList>{entries}</CommentList>"))
    };
    let (_, plain) = read_ok(&package(Some(&comment_with(""))));
    let (doc, heavy) = read_ok(&package(Some(&comment_with(&attrs(32, 4096)))));
    assert_eq!(doc.comments.len(), 20);
    let kept: u64 = 20 * 32 * 4096;
    assert!(
        heavy.stats.memory_bytes >= plain.stats.memory_bytes + kept,
        "the 20 x 32 kept attributes of 4 KB ({kept} bytes) are charged: {} against {}",
        heavy.stats.memory_bytes,
        plain.stats.memory_bytes
    );
}

#[test]
fn more_kept_attributes_than_an_entry_may_hold_leave_the_part_as_bytes() {
    let attrs: String = (0..200).map(|i| format!(r#" X{i}="1""#)).collect();
    let body = format!(r#"<CommentList><CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="1"{attrs}>c</CommentEntry></CommentList>"#);
    let (_, report) = held_as_bytes(&comments_xml(&body));
    assert_eq!(report.warning_count("comments_unreadable"), 1);
    let authors: String = (0..200).map(|i| format!(r#" X{i}="1""#)).collect();
    let body = format!(r#"<AuthorList><AuthorEntry ID="1"{authors}/></AuthorList>"#);
    let (_, report) = held_as_bytes(&comments_xml(&body));
    assert_eq!(report.warning_count("comments_unreadable"), 1);
}

#[test]
fn comments_with_a_hundred_thousand_attributes_of_a_kilobyte_are_not_read_into_the_model() {
    // 100,000 attributes of 1 KB each are 100 MB of part: far past the size of a small part, so the part stays the bytes it is and no entry is looked at.
    let extra = format!(r#" K="{}""#, "v".repeat(1024));
    let mut list = String::from("<CommentList>");
    let mut n = 0_u32;
    while list.len() < 5 * 1024 * 1024 {
        n += 1;
        list.push_str(&format!(r#"<CommentEntry AuthorID="1" PageID="1" Date="d" CommentID="{n}"{extra}>c</CommentEntry>"#));
    }
    list.push_str("</CommentList>");
    work::reset();
    let (_, report) = held_as_bytes(&comments_xml(&list));
    assert_eq!(report.warning_count("comments_unreadable"), 1);
    assert_eq!(work::comment_entries_read(), 0);
}

// ------------------------------------------------------------------------------------------------------- the part as it was read (the sample-file check of GATE.ea42)

/// The text of the model writer's comments part.
fn written_part(doc: &Document) -> String {
    let written = write_with(doc, None, &WriteOptions::default()).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(part(&written.bytes, "/visio/comments.xml").expect("a comments part")).unwrap()
}

/// The start tag of the author entry numbered `id` in `xml`.
fn author_tag(xml: &str, id: u32) -> &str {
    let at = xml.find(&format!("<AuthorEntry ID=\"{id}\"")).unwrap_or_else(|| panic!("no author {id} in {xml}"));
    &xml[at..at + xml[at..].find('>').unwrap()]
}

const BARE_AUTHORS: &str = r#"<AuthorList><AuthorEntry ID="1" Name="Ada"/><AuthorEntry ID="2" Name="Bob" Initials=""/><AuthorEntry ID="3" Name="Cy" Initials="C"/><AuthorEntry ID="4" Initials="D"/></AuthorList><CommentList><CommentEntry AuthorID="1" PageID="1" Date="2031-03-07T20:05:09Z" CommentID="1">plain</CommentEntry></CommentList>"#;

#[test]
fn an_author_with_no_initials_is_written_with_none_and_an_empty_initials_only_where_the_file_had_one() {
    let (doc, _) = read_ok(&package(Some(&comments_xml(BARE_AUTHORS))));
    let xml = written_part(&doc);
    assert!(!author_tag(&xml, 1).contains("Initials"), "no attribute the file lacked: {xml}");
    assert!(author_tag(&xml, 2).contains("Initials=\"\""), "an empty one the file had: {xml}");
    assert!(author_tag(&xml, 3).contains("Initials=\"C\""));
    assert!(author_tag(&xml, 4).contains("Initials=\"D\"") && !author_tag(&xml, 4).contains("Name"), "a name the file lacked: {xml}");
    // And what is written reads back as the model it was written from.
    let (back, _) = read_ok(&package(Some(&xml)));
    assert_eq!(back.comments, doc.comments);
}

#[test]
fn the_root_has_xml_space_where_the_file_had_it_and_where_a_text_needs_it() {
    // A file with none, and plain texts: none is written.
    let (plain, _) = read_ok(&package(Some(&comments_xml(BARE_AUTHORS))));
    assert!(!written_part(&plain).contains("xml:space"), "{}", written_part(&plain));
    // A file that had it keeps it, whatever the texts.
    let with = format!(r#"<Comments xmlns="{NS_2012}" xmlns:r="{NS_R}" xml:space="preserve">{BARE_AUTHORS}</Comments>"#);
    let (had, _) = read_ok(&package(Some(&with)));
    assert!(written_part(&had).contains("xml:space=\"preserve\""), "{}", written_part(&had));
    // A part read from a file that had none is written with none, whatever a text says (a text added to it too): the file's own comments were written that way.
    for text in ["two\nlines", "a\ttab", "two  blanks", " leading", "trailing "] {
        let mut doc = plain.clone();
        let author = doc.comments.authors().next().unwrap().id;
        doc.comments
            .add(NewComment { parent: None, page: 1, shape: None, author, text: Arc::from(text), date: Arc::from("2031-04-01T00:00:00Z") })
            .unwrap();
        assert!(!written_part(&doc).contains("xml:space"), "{text:?}");
    }
    // A part made new has it only where a text needs it: a line break, a tab, a run of blanks or a blank at an end.
    let new_with = |text: &str| {
        let mut fresh = Document::new_blank();
        let author = fresh.comments.author_for("Ada", "A").unwrap();
        fresh
            .comments
            .add(NewComment { parent: None, page: 1, shape: None, author, text: Arc::from(text), date: Arc::from("2031-04-01T00:00:00Z") })
            .unwrap();
        written_part(&fresh)
    };
    assert!(!new_with("plain text").contains("xml:space"));
    for text in ["two\nlines", "a\ttab", "two  blanks", " leading", "trailing "] {
        assert!(new_with(text).contains("xml:space=\"preserve\""), "{text:?}");
    }
}
