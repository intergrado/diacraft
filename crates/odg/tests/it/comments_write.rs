//! The writers' side: the document's comments are a page's `office:annotation` elements in both `.odg` writers (the model writer, `write_with(doc, None, ..)`, and the
//! splice into a file), written from the model and read back by the real reader. Inputs are built in code from ODF 1.4 Part 3 (`office:annotation`, §14.1); no file made by
//! another program is read.
//!
//! What is held: a comment on the page, on a shape, a reply, a Done mark and an edit date come back as they were; a comment of the model is written at its marker; a shape a comment
//! is on keeps its number; a page whose comments are as the file has them keeps their bytes; an annotation the model cannot hold exactly stays where it is.

use std::sync::Arc;

use diacraft_doc::{Comment, Document, NewComment};
use diacraft_odg::{Limits, WriteError, WriteOptions, Written, read};

use crate::model_write::{blank, save};
use crate::write_core::{Open, open, part, real};
use crate::write_new::{push_shape, rectangle};

/// The length in centimetres an attribute of the first `tag` of `xml` holds.
fn attr_cm(xml: &str, tag: &str, name: &str) -> Option<f64> {
    let start = xml.find(tag)?;
    let head = &xml[start..start + xml[start..].find('>')?];
    let at = head.find(&format!(" {name}=\""))? + name.len() + 3;
    let value = &head[at..at + head[at..].find('"')?];
    match value.strip_suffix("cm") {
        Some(cm) => cm.parse().ok(),
        None => value.strip_suffix("in")?.parse::<f64>().ok().map(|inches| inches * 2.54),
    }
}

/// The place (x, y, in centimetres) each `office:annotation` of `xml` gives.
fn annotation_places(xml: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<office:annotation ") {
        rest = &rest[at..];
        let x = attr_cm(rest, "<office:annotation ", "svg:x").expect("an x");
        let y = attr_cm(rest, "<office:annotation ", "svg:y").expect("a y");
        out.push((x, y));
        rest = &rest["<office:annotation ".len()..];
    }
    out
}

/// A comment of the model: its author's name and initials, its text, and where it is.
fn add(doc: &mut Document, page: u32, shape: Option<u32>, parent: Option<u32>, who: &str, text: &str) -> u32 {
    let initials: String = who.chars().take(2).collect();
    let author = doc.comments.author_for(who, &initials).expect("an author");
    doc.comments.add(NewComment { parent, page, shape, author, text: Arc::from(text), date: Arc::from("2031-03-07T20:05:09Z") }).expect("a comment")
}

/// What a comment says that a file holds: its number, where it is, who wrote it, its text and its dates.
type Said = (u32, Option<u32>, Option<u32>, String, String, String, String, bool);

fn said(doc: &Document) -> Vec<Said> {
    doc.comments
        .iter()
        .map(|c: &Arc<Comment>| {
            let author = doc.comments.author(c.author).expect("an author");
            (
                c.id,
                c.shape,
                c.parent,
                author.name.to_string(),
                author.initials.to_string(),
                c.text.to_string(),
                format!("{}|{}", c.date, c.edit_date.as_deref().unwrap_or("")),
                c.done,
            )
        })
        .collect()
}

fn reread(bytes: &[u8]) -> Document {
    read(bytes, &Limits::default()).unwrap_or_else(|e| panic!("the file did not read: {} ({e})", e.class())).0
}

#[test]
fn the_model_writer_writes_a_page_s_comments_and_the_reader_reads_the_same() {
    let mut doc = blank(2);
    let page = doc.pages.get(0).unwrap().id;
    let a = add(&mut doc, page, None, None, "Ada", "a note on the page");
    let b = add(&mut doc, page, Some(2), None, "Bob", "a note on the second rectangle\nwith two lines");
    let _reply = add(&mut doc, page, Some(2), Some(b), "Ada", "an answer");
    doc.comments.set_done(a, true, Arc::from("2031-04-01T00:00:00Z")).unwrap();
    doc.comments.edit(b, "a note on the second rectangle\nwith two lines, edited", Arc::from("2031-05-01T00:00:00Z")).unwrap();
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    let content = part(&saved.bytes, "content.xml");
    assert_eq!(content.matches("<office:annotation ").count(), 3, "{content}");
    // The schema's place and order: after the shapes, and the fields in the order of the schema.
    assert!(content.find("</draw:custom-shape>").or_else(|| content.rfind("</draw:rect>")).unwrap() < content.find("<office:annotation ").unwrap());
    assert!(content.contains("<dc:creator>Ada</dc:creator><dc:date>2031-03-07T20:05:09Z</dc:date><meta:creator-initials>Ad</meta:creator-initials><text:p>a note on the page</text:p>"));
    let back = reread(&saved.bytes);
    assert_eq!(said(&back), said(&doc));
}

#[test]
fn a_page_with_comments_and_no_shape_is_written_whole_and_reads_back() {
    let mut doc = blank(0);
    let page = doc.pages.get(0).unwrap().id;
    add(&mut doc, page, None, None, "Ada", "alone on an empty page");
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    let content = part(&saved.bytes, "content.xml");
    assert!(content.contains("</office:annotation></draw:page>"), "{content}");
    assert_eq!(said(&reread(&saved.bytes)), said(&doc));
}

#[test]
fn a_comment_the_model_made_is_written_at_the_marker_of_its_shape_or_inside_the_page_s_corner() {
    let mut doc = blank(1);
    let page = doc.pages.get(0).unwrap().id;
    add(&mut doc, page, Some(1), None, "Ada", "on the shape");
    add(&mut doc, page, None, None, "Ada", "on the page");
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    let content = part(&saved.bytes, "content.xml");
    let (shape_x, shape_y, shape_w) = attr_cm(&content, "<draw:custom-shape", "svg:x")
        .zip(attr_cm(&content, "<draw:custom-shape", "svg:y"))
        .zip(attr_cm(&content, "<draw:custom-shape", "svg:width"))
        .map(|((x, y), w)| (x, y, w))
        .expect("the shape's box");
    let notes = annotation_places(&content);
    assert_eq!(notes.len(), 2);
    // The top right corner of the shape's box, from the page's top.
    assert!(
        (notes[0].0 - (shape_x + shape_w)).abs() < 0.01 && (notes[0].1 - shape_y).abs() < 0.01,
        "{notes:?} against the shape at {shape_x}, {shape_y}, {shape_w}"
    );
    // The page's top right corner, 0.3 in inside it: A4 is 21 cm wide.
    assert!((notes[1].0 - (21.0 - 0.3 * 2.54)).abs() < 0.01 && (notes[1].1 - 0.3 * 2.54).abs() < 0.01, "{notes:?}");
}

#[test]
fn a_shape_a_comment_is_on_keeps_its_number_whatever_the_numbers_of_the_others() {
    // The shapes are numbered 3 and 7 (a drawing whose first shapes were taken away); an `.odg` numbers shapes in document order unless one is named, so the one the comment
    // is on is named: it comes back as 7, and the other as the reader numbers it.
    let mut doc = blank(0);
    push_shape(&mut doc, rectangle(3, "Three", (2.0, 3.0, 2.0, 2.0)));
    push_shape(&mut doc, rectangle(7, "Seven", (6.0, 3.0, 2.0, 2.0)));
    let page = doc.pages.get(0).unwrap().id;
    add(&mut doc, page, Some(7), None, "Ada", "on seven");
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    let back = reread(&saved.bytes);
    let c = back.comments.iter().next().expect("a comment");
    assert_eq!(c.shape, Some(7));
    let tied = back.pages.get(0).unwrap().shapes.iter().find(|s| s.id == 7).expect("a shape 7");
    assert_eq!(tied.name.as_deref(), Some("Seven"), "the number is the shape's: the comment is on the shape it was on");
}

#[test]
fn text_with_runs_of_spaces_tabs_empty_lines_and_markup_characters_reads_back_as_it_was() {
    let mut doc = blank(1);
    let page = doc.pages.get(0).unwrap().id;
    for text in [
        "  leading and trailing  ",
        "a  b   c",
        "tab\there",
        "first\n\nthird after an empty line",
        "a < b && c > \"d\" 'e'",
        "ünïcödé — 日本語 \u{1F600}",
        "x\n",
    ] {
        add(&mut doc, page, None, None, "Ada", text);
    }
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    assert_eq!(said(&reread(&saved.bytes)), said(&doc));
}

#[test]
fn a_comment_with_a_character_xml_cannot_hold_is_refused_in_words() {
    let mut doc = blank(1);
    let page = doc.pages.get(0).unwrap().id;
    add(&mut doc, page, None, None, "Ada", "fine");
    // A model made by a reader that does not check characters (`Comments::from_parts`) may hold one.
    let mut all: Vec<Comment> = doc.comments.iter().map(|c| (**c).clone()).collect();
    all[0].text = Arc::from("bell \u{7} here");
    let authors: Vec<_> = doc.comments.authors().map(|a| (**a).clone()).collect();
    doc.comments = diacraft_doc::Comments::from_parts(None, diacraft_doc::RawAttrs::new(), authors, all).unwrap();
    match save(&doc, &WriteOptions::default()) {
        Err(WriteError::Unsupported(words)) => assert!(words.contains("a character XML 1.0 does not allow"), "{words}"),
        other => panic!("expected a refusal in words, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------------- the splice

/// A drawing of one page of the custom shapes `s1` to `s3`, opened, with the comments `build` adds.
fn opened_with(build: impl FnOnce(&mut Document, u32)) -> Open {
    let mut o = open(&crate::write_core::drawing_of(3));
    let page = o.doc.pages.get(0).unwrap().id;
    build(&mut o.doc, page);
    o
}

fn saved(o: &Open) -> Written {
    real(&o.doc, &o.bytes, &o.index).unwrap_or_else(|e| panic!("the save was refused: {} ({e})", e.class()))
}

#[test]
fn a_comment_added_to_a_file_is_spliced_in_at_the_end_of_its_page_and_nothing_else_changes() {
    let o = opened_with(|doc, page| {
        add(doc, page, Some(2), None, "Ada", "on the second shape");
        add(doc, page, None, None, "Bob", "on the page");
    });
    let s = saved(&o);
    assert!(s.edited);
    let (before, after) = (part(&o.bytes, "content.xml"), part(&s.bytes, "content.xml"));
    let at = after.find("<office:annotation ").expect("an annotation");
    // Everything before the annotations and after them is the file's own bytes.
    assert!(after.starts_with(&before[..before.find("</draw:page>").unwrap()]), "the page's own markup is as it was");
    assert!(after.ends_with(&before[before.find("</draw:page>").unwrap()..]));
    assert!(at > before.find("</draw:custom-shape>").unwrap());
    for name in ["styles.xml", "settings.xml"] {
        if crate::write_core::has_part(&o.bytes, name) {
            assert_eq!(part(&o.bytes, name), part(&s.bytes, name), "{name}");
        }
    }
    let back = reread(&s.bytes);
    assert_eq!(said(&back), said(&o.doc));
    // The shape is the file's second: its number is what the reader gave it, and no number is written on it.
    assert_eq!(back.comments.iter().next().and_then(|c| c.shape), Some(2));
    assert!(!after.contains("diacraft:id=\"2\" "), "no shape is named for it");
}

#[test]
fn a_second_save_of_what_a_save_wrote_is_no_edit() {
    let o = opened_with(|doc, page| {
        add(doc, page, Some(3), None, "Ada", "tied to the third");
        let first = add(doc, page, None, None, "Bob", "a thread");
        add(doc, page, None, Some(first), "Ada", "a reply");
    });
    let s = saved(&o);
    let again = real(&o.doc, &s.bytes, &s.index).expect("the second save");
    assert!(!again.edited, "a second save of what a save wrote is no edit");
    assert_eq!(again.bytes, s.bytes);
    // And a session that reopens the file holds the same comments, and saves it again as no edit either.
    let reopened = crate::write_core::open_bytes(s.bytes.clone());
    assert_eq!(said(&reopened.doc), said(&o.doc));
    let third = real(&reopened.doc, &reopened.bytes, &reopened.index).expect("the third save");
    assert!(!third.edited);
}

#[test]
fn the_annotations_of_a_file_keep_their_bytes_while_its_comments_are_as_they_were() {
    let notes = r#"<officeooo:annotation svg:x="0.6cm" svg:y="0.3cm" svg:width="0.5cm" svg:height="0.3cm"><dc:creator>Ada Lovelace</dc:creator><meta:creator-initials>AL</meta:creator-initials><dc:date>2031-03-07T20:05:09</dc:date><text:p>First comment</text:p></officeooo:annotation>"#;
    let mut d = crate::write_core::drawing_of(2);
    d.ns = format!("{} xmlns:officeooo=\"http://openoffice.org/2009/office\"", d.ns);
    d.pages = d.pages.replace("</draw:page>", &format!("{notes}</draw:page>"));
    let mut o = open(&d);
    assert_eq!(o.doc.comments.len(), 1);
    // An edit that is no comment's: a shape moves.
    crate::write_core::set_num(crate::write_core::shape_mut(&mut o.doc, 0, 1), "PinX", 3.0);
    let s = saved(&o);
    assert!(s.edited);
    assert!(part(&s.bytes, "content.xml").contains(notes), "the annotation is the file's bytes");
}

#[test]
fn a_comment_taken_away_takes_its_annotation_out_and_leaves_the_ones_the_model_cannot_hold() {
    let rich = r#"<office:annotation><dc:creator>Zed</dc:creator><text:p>a <text:span text:style-name="T1">styled</text:span> word</text:p></office:annotation>"#;
    let plain = |who: &str, text: &str| format!("<office:annotation><dc:creator>{who}</dc:creator><text:p>{text}</text:p></office:annotation>");
    let mut d = crate::write_core::drawing_of(1);
    d.pages = d.pages.replace("</draw:page>", &format!("{}{rich}{}</draw:page>", plain("Ada", "one"), plain("Bob", "two")));
    let mut o = open(&d);
    assert_eq!(o.doc.comments.len(), 2);
    let first = o.doc.comments.iter().next().unwrap().id;
    o.doc.comments.remove(first).unwrap();
    let s = saved(&o);
    let content = part(&s.bytes, "content.xml");
    assert!(!content.contains("<text:p>one</text:p>"));
    assert!(content.contains(rich), "the annotation the model could not hold is untouched");
    assert_eq!(content.matches("<office:annotation").count(), 2, "the rich one and the second comment, written anew");
    let back = reread(&s.bytes);
    assert_eq!(back.comments.len(), 1);
    assert_eq!(back.comments.iter().next().map(|c| c.text.to_string()), Some("two".to_owned()));
    let again = real(&o.doc, &s.bytes, &s.index).expect("the second save");
    assert!(!again.edited, "and the next save is no edit");
}

#[test]
fn a_comment_edited_and_one_marked_done_are_written_again_and_read_back() {
    let o = opened_with(|doc, page| {
        add(doc, page, None, None, "Ada", "first");
        add(doc, page, None, None, "Bob", "second");
    });
    let s = saved(&o);
    let mut o2 = crate::write_core::open_bytes(s.bytes.clone());
    let id = o2.doc.comments.iter().next().unwrap().id;
    o2.doc.comments.edit(id, "first, edited", Arc::from("2031-06-01T00:00:00Z")).unwrap();
    let last = o2.doc.comments.iter().next_back().unwrap().id;
    o2.doc.comments.set_done(last, true, Arc::from("2031-06-02T00:00:00Z")).unwrap();
    let s2 = saved(&o2);
    assert!(s2.edited);
    assert_eq!(said(&reread(&s2.bytes)), said(&o2.doc));
}

#[test]
fn a_comment_on_a_shape_of_the_file_is_tied_to_it_and_a_save_that_renumbers_the_shapes_ties_it_to_the_page_until_the_next() {
    // Comment on the third shape; the first is taken out in the same save, so the third is read as the second: a tie of 3 would be another shape's.
    let mut o = opened_with(|doc, page| {
        add(doc, page, Some(3), None, "Ada", "on the third");
    });
    let page = Arc::make_mut(o.doc.pages.get_mut(0).unwrap());
    let at = page.shapes.iter().position(|s| s.id == 1).unwrap();
    page.shapes.remove(at);
    let s = saved(&o);
    let back = reread(&s.bytes);
    assert_eq!(back.comments.iter().next().and_then(|c| c.shape), None, "written on the page, never tied to another shape");
    // The model still has it on the shape, and the next save that leaves the numbering alone ties it again, to the number the shape is read with.
    let next = real(&o.doc, &s.bytes, &s.index).expect("the next save");
    assert!(next.edited);
    let tied = reread(&next.bytes);
    let shape = tied.comments.iter().next().and_then(|c| c.shape).expect("tied again");
    let name = tied.pages.get(0).unwrap().shapes.iter().find(|x| x.id == shape).and_then(|x| x.name.clone());
    assert_eq!(name.as_deref(), Some("s3"), "the shape it is tied to is the third of the file");
    let last = real(&o.doc, &next.bytes, &next.index).expect("the last save");
    assert!(!last.edited);
}

// ------------------------------------------------------------------------------------------------------ the security scan's finding: what the text's shape makes

/// The bytes of the annotations in a `content.xml`.
fn annotation_bytes(content: &str) -> u64 {
    let (from, to) = (
        content.find("<office:annotation ").expect("an annotation"),
        content.rfind("</office:annotation>").expect("an end") + "</office:annotation>".len(),
    );
    (to - from) as u64
}

/// A text of 64 KiB made of `unit` and nothing else (a comment is at most that long).
fn of_unit(unit: &str) -> String {
    unit.repeat(diacraft_doc::MAX_COMMENT_TEXT_BYTES / unit.len())
}

#[test]
fn a_comment_of_newlines_tabs_or_spaces_is_written_within_what_the_save_charged_for_it() {
    // Each newline is a paragraph, each tab a `text:tab` and each run of two spaces a `text:s`: far more than the text's length, and what the save held for them is what they come to.
    for (what, text) in [
        ("newlines", of_unit("\n").replacen('\n', "x", 1)),
        ("tabs", format!("x{}", "\t".repeat(diacraft_doc::MAX_COMMENT_TEXT_BYTES - 1))),
        ("alternating spaces", of_unit("a  ")),
        ("single spaces between marks", of_unit("<  ")),
        ("one long run", format!("x{}x", " ".repeat(diacraft_doc::MAX_COMMENT_TEXT_BYTES - 2))),
    ] {
        let mut doc = blank(1);
        let page = doc.pages.get(0).unwrap().id;
        add(&mut doc, page, Some(1), None, "Ada", &text);
        match save(&doc, &WriteOptions::default()) {
            Ok(saved) => {
                let written = annotation_bytes(&part(&saved.bytes, "content.xml"));
                assert!(written <= saved.work.comment_bytes_held, "{what}: {written} bytes written, {} held", saved.work.comment_bytes_held);
                assert_eq!(said(&reread(&saved.bytes)), said(&doc), "{what}: and it reads back");
            }
            Err(WriteError::Limit { what: words, .. }) => assert!(!words.is_empty(), "{what}: refused in words"),
            Err(other) => panic!("{what}: {other:?}"),
        }
    }
}

#[test]
fn a_hundred_thousand_comments_are_written_within_what_the_save_held_and_read_back() {
    // 100,000 comments (the most a drawing holds) on a page, each with an author, a date and two lines: charged by the shape of what is written, and counted in steps.
    let mut doc = blank(1);
    let page = doc.pages.get(0).unwrap().id;
    let authors: Vec<diacraft_doc::CommentAuthor> = (1..=50_u32)
        .map(|id| diacraft_doc::CommentAuthor {
            id,
            name: Arc::from(format!("Author {id}").as_str()),
            initials: Arc::from("A"),
            raw: diacraft_doc::RawAttrs::new(),
        })
        .collect();
    let list: Vec<Comment> = (1..=100_000_u32)
        .map(|id| Comment {
            id,
            parent: (id % 3 == 0).then(|| id - 2),
            page,
            shape: Some(1),
            author: 1 + id % 50,
            text: Arc::from(format!("comment {id}\nsecond line").as_str()),
            date: Arc::from("2031-03-07T20:05:09Z"),
            edit_date: None,
            done: false,
            raw: diacraft_doc::RawAttrs::new(),
        })
        .collect();
    doc.comments = diacraft_doc::Comments::from_parts(None, diacraft_doc::RawAttrs::new(), authors, list).unwrap();
    let saved = save(&doc, &WriteOptions::default()).expect("saved");
    let written = annotation_bytes(&part(&saved.bytes, "content.xml"));
    assert!(written <= saved.work.comment_bytes_held, "{written} bytes written, {} held", saved.work.comment_bytes_held);
    // Steps in proportion to the comments, not to their square: a few for each comment.
    assert!(saved.work.cells_compared < 100_000 * 8, "{} steps", saved.work.cells_compared);
    let back = reread(&saved.bytes);
    assert_eq!(back.comments.len(), 100_000);
    assert_eq!(back.comments.iter().filter(|c| c.parent.is_some()).count(), 33_333);
}

#[test]
fn a_flat_drawing_holds_its_comments_the_same_way_and_a_second_save_is_no_edit() {
    let own = r#"<office:annotation svg:x="12cm" svg:y="1cm"><dc:creator>Eve</dc:creator><text:p>the file's own</text:p></office:annotation>"#;
    let mut d = crate::write_core::drawing_of(2);
    d.pages = d.pages.replace("</draw:page>", &format!("{own}</draw:page>"));
    let flat = crate::flat::Flat::of(&d);
    let mut o = crate::write_core::open_bytes(flat.bytes());
    assert_eq!(o.doc.comments.len(), 1);
    let page = o.doc.pages.get(0).unwrap().id;
    add(&mut o.doc, page, Some(2), None, "Dee", "added to a flat file");
    let s = saved(&o);
    assert!(s.edited);
    let text = String::from_utf8(s.bytes.clone()).unwrap();
    assert!(text.contains("added to a flat file"));
    assert_eq!(said(&reread(&s.bytes)), said(&o.doc));
    let again = real(&o.doc, &s.bytes, &s.index).expect("the second save");
    assert!(!again.edited, "a second save of what a save wrote is no edit");
}

#[test]
#[ignore = "writes the files the black-box check with LibreOffice loads: set DIACRAFT_ODG_COMMENTS_OUT to a folder of your own"]
fn writes_the_files_a_black_box_check_loads() {
    let Ok(dir) = std::env::var("DIACRAFT_ODG_COMMENTS_OUT") else { return };
    // From the model: a comment on the page, one on each of two shapes, a reply, and a Done mark.
    let mut doc = blank(2);
    let page = doc.pages.get(0).unwrap().id;
    let a = add(&mut doc, page, None, None, "Ada Lovelace", "A note on the page\nin two lines");
    let b = add(&mut doc, page, Some(1), None, "Bob", "On the first rectangle");
    add(&mut doc, page, Some(1), Some(b), "Ada Lovelace", "A reply to Bob");
    add(&mut doc, page, Some(2), None, "Cy", "On the second one & <marked>");
    doc.comments.set_done(a, true, Arc::from("2031-04-01T00:00:00Z")).unwrap();
    let written = save(&doc, &WriteOptions::default()).expect("saved");
    std::fs::write(format!("{dir}/model.odg"), &written.bytes).unwrap();
    // Spliced into a file: the file's own annotation and a comment added to its shape.
    let kept = r#"<office:annotation svg:x="12cm" svg:y="1cm"><dc:creator>Eve</dc:creator><dc:date>2031-03-07T20:05:09</dc:date><text:p>The file's own comment</text:p></office:annotation>"#;
    let mut d = crate::write_core::drawing_of(3);
    d.pages = d.pages.replace("</draw:page>", &format!("{kept}</draw:page>"));
    let mut file = open(&d);
    let page = file.doc.pages.get(0).unwrap().id;
    add(&mut file.doc, page, Some(2), None, "Dee", "Added to the second shape of a file");
    std::fs::write(format!("{dir}/splice.odg"), &saved(&file).bytes).unwrap();
}
