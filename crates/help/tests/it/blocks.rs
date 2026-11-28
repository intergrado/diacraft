//! The blocks of Help Markdown (design section 2.3): each one parsed, and each refusal with its line.

use diacraft_help::{Block, Code, CodeLang, List, Span, Text};

use crate::{problems, read};

fn blocks(body: &str) -> Vec<Block> {
    let parsed = read(body);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    parsed.topic.doc.blocks
}

fn plain(text: &Text) -> String {
    fn walk(spans: &[Span], out: &mut String) {
        for span in spans {
            match span {
                Span::Text(t) | Span::Code(t) => out.push_str(t),
                Span::Strong(s) | Span::Em(s) => walk(s, out),
                Span::Link { text, .. } => walk(text, out),
            }
        }
    }
    let mut out = String::new();
    walk(&text.spans, &mut out);
    out
}

fn items(list: &List) -> Vec<String> {
    list.items.iter().map(|i| i.text.source.clone()).collect()
}

#[test]
fn headings_have_two_levels_and_an_anchor_from_their_text_or_given() {
    let got = blocks("## Hello World\n\n### Deep one {#deep-anchor}\n\n## Snap & Glue\n");
    let shape: Vec<(u8, String, String)> = got
        .iter()
        .map(|b| match b {
            Block::Heading { level, text, anchor } => (*level, text.source.clone(), anchor.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        shape,
        [
            (2, "Hello World".to_owned(), "hello-world".to_owned()),
            (3, "Deep one".to_owned(), "deep-anchor".to_owned()),
            (2, "Snap & Glue".to_owned(), "snap-glue".to_owned()),
        ]
    );
}

#[test]
fn an_anchor_comes_from_the_text_a_person_reads_and_not_from_its_markup() {
    let got = blocks("## Using `Cmd+Z` with *care*\n");
    match &got[0] {
        Block::Heading { anchor, text, .. } => {
            assert_eq!(anchor, "using-cmd-z-with-care");
            assert_eq!(text.source, "Using `Cmd+Z` with *care*");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn headings_the_format_refuses_are_found_with_their_lines() {
    assert_eq!(problems("# Title\n"), [(1, Code::HeadingLevel)]);
    assert_eq!(problems("para\n\n#### Four\n"), [(3, Code::HeadingLevel)]);
    assert_eq!(problems("##\n"), [(1, Code::HeadingEmpty)]);
    assert_eq!(problems("## A {#bad anchor}\n"), [(1, Code::AnchorSyntax)]);
    assert_eq!(problems("## A {#Upper}\n"), [(1, Code::AnchorSyntax)]);
    assert_eq!(problems("## …\n"), [(1, Code::AnchorEmpty)]);
    assert!(problems("## … {#dots}\n").is_empty());
    assert_eq!(problems("## Same\n\ntext\n\n## Same\n"), [(5, Code::AnchorDuplicate)]);
    // a text that only looks like a heading is text
    assert!(problems("##hashtag\n").is_empty());
}

#[test]
fn a_duplicate_anchor_is_made_unique_so_that_the_topic_still_reads() {
    let parsed = read("## Same\n\n## Same\n");
    let anchors: Vec<_> =
        parsed.topic.doc.blocks.iter().filter_map(|b| if let Block::Heading { anchor, .. } = b { Some(anchor.clone()) } else { None }).collect();
    assert_eq!(anchors, ["same", "same-2"]);
}

#[test]
fn a_paragraph_joins_its_lines_with_one_space() {
    let got = blocks("one  \ntwo\nthree\n\nnext\n");
    match (&got[0], &got[1]) {
        (Block::Para(a), Block::Para(b)) => assert_eq!((a.source.as_str(), b.source.as_str()), ("one two three", "next")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_block_ends_at_a_blank_line_and_at_a_line_of_another_kind() {
    let got = blocks("para\n- item\n> note\n| a |\n|---|\n| b |\n```text\ncode\n```\ntext after\n");
    let kinds: Vec<&str> = got
        .iter()
        .map(|b| match b {
            Block::Para(_) => "para",
            Block::List(_) => "list",
            Block::Note(_) => "note",
            Block::Table { .. } => "table",
            Block::Code { .. } => "code",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["para", "list", "note", "table", "code", "para"]);
}

#[test]
fn lists_are_bulleted_or_numbered_with_one_nested_level_and_continued_lines() {
    let got = blocks("- one\n- two\n  continues here\n  - nested a\n  - nested b\n    continued\n- three\n\n1. first\n2. second\n  - under\n");
    let (Block::List(bullets), Block::List(numbers)) = (&got[0], &got[1]) else { panic!("{got:?}") };
    assert!(!bullets.ordered && numbers.ordered);
    assert_eq!(items(bullets), ["one", "two continues here", "three"]);
    let sub = bullets.items[1].sub.as_ref().unwrap();
    assert!(!sub.ordered);
    assert_eq!(items(sub), ["nested a", "nested b continued"]);
    assert!(sub.items.iter().all(|i| i.sub.is_none()));
    assert_eq!(items(numbers), ["first", "second"]);
    assert_eq!(items(numbers.items[1].sub.as_ref().unwrap()), ["under"]);
}

#[test]
fn a_nested_list_may_be_numbered_under_bullets() {
    let got = blocks("- a\n  1. x\n  2. y\n");
    let Block::List(list) = &got[0] else { panic!("{got:?}") };
    let sub = list.items[0].sub.as_ref().unwrap();
    assert!(sub.ordered);
    assert_eq!(items(sub), ["x", "y"]);
}

#[test]
fn a_third_level_is_refused_at_its_line_and_flattened_to_the_second() {
    let src = "- a\n  - b\n    - c\n";
    assert_eq!(problems(src), [(3, Code::ListTooDeep)]);
    let got = read(src).topic.doc.blocks;
    let Block::List(list) = &got[0] else { panic!("{got:?}") };
    let sub = list.items[0].sub.as_ref().unwrap();
    assert_eq!(items(sub), ["b", "c"]);
    assert!(sub.items.iter().all(|i| i.sub.is_none()));
}

#[test]
fn list_lines_with_the_wrong_indent_and_mixed_markers_are_refused() {
    assert_eq!(problems("- a\n - b\n"), [(2, Code::ListIndent)]);
    assert_eq!(problems("- a\n   - b\n"), [(2, Code::ListIndent)]);
    assert_eq!(problems("- a\n   continued\n"), [(2, Code::ListIndent)]);
    assert_eq!(problems("- a\n1. b\n"), [(2, Code::ListMarkerMix)]);
    assert_eq!(problems("- a\n  - b\n  1. c\n"), [(3, Code::ListMarkerMix)]);
}

#[test]
fn a_list_ends_at_a_blank_line_and_what_is_indented_after_it_is_refused() {
    assert_eq!(problems("- a\n\n  - b\n"), [(3, Code::IndentedLine)]);
    assert_eq!(problems("  - b\n"), [(1, Code::IndentedLine)]);
}

#[test]
fn a_note_is_one_box_of_lines_joined_with_a_space() {
    let got = blocks("> first\n> second\n>third\n");
    let Block::Note(note) = &got[0] else { panic!("{got:?}") };
    assert_eq!(note.source, "first second third");
}

#[test]
fn a_note_in_a_note_or_with_an_empty_line_is_refused() {
    assert_eq!(problems("> a\n> > b\n"), [(2, Code::NoteNested)]);
    assert_eq!(problems("> a\n>\n> b\n"), [(2, Code::NoteBlank)]);
}

#[test]
fn a_code_block_is_verbatim_and_never_read_as_markup() {
    let src = "```sh\ndiacraft-cli run --cmd file.new\n\n  *not em* <b> # not a heading\n- not a list\n```\n\n```\nplain\n```\n\n```json\n{\"a\": 1}\n```\n";
    let got = blocks(src);
    let langs: Vec<CodeLang> = got
        .iter()
        .map(|b| match b {
            Block::Code { lang, .. } => *lang,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(langs, [CodeLang::Sh, CodeLang::Text, CodeLang::Json]);
    match &got[0] {
        Block::Code { text, .. } => assert_eq!(text, "diacraft-cli run --cmd file.new\n\n  *not em* <b> # not a heading\n- not a list"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_code_block_never_closed_and_one_in_another_language_are_refused() {
    assert_eq!(problems("para\n\n```sh\nopen\n"), [(3, Code::CodeUnclosed)]);
    assert_eq!(problems("```rust\nx\n```\n"), [(1, Code::CodeLanguage)]);
    // an unclosed block runs to the end of the topic
    let got = read("```text\nopen\nmore\n").topic.doc.blocks;
    match &got[0] {
        Block::Code { text, .. } => assert_eq!(text, "open\nmore"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_table_has_a_header_a_separator_and_rows_of_inlines() {
    let got = blocks("| Key | Does |\n|---|:---:|\n| **Alt+Q** | Opens `search` |\n| F1 | Opens help \\| now |\n");
    let Block::Table { header, rows } = &got[0] else { panic!("{got:?}") };
    assert_eq!(header.iter().map(|c| c.source.as_str()).collect::<Vec<_>>(), ["Key", "Does"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0].source, "**Alt+Q**");
    assert!(matches!(rows[0][0].spans.as_slice(), [Span::Strong(_)]));
    assert_eq!(rows[1][1].source, "Opens help | now");
    assert_eq!(plain(&rows[1][1]), "Opens help | now");
}

#[test]
fn a_table_the_format_refuses_is_found_with_its_line() {
    assert_eq!(problems("| a | b |\n| c | d |\n"), [(2, Code::TableSeparator)]);
    assert_eq!(problems("| a | b |\n|---|---|\n| c |\n"), [(3, Code::TableShape)]);
    assert_eq!(problems("| a | b |\n|---|\n| c | d |\n"), [(2, Code::TableShape)]);
    let wide = "| 1 | 2 | 3 | 4 | 5 | 6 | 7 |\n|---|---|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 | 6 | 7 |\n";
    assert_eq!(problems(wide), [(1, Code::TableTooWide)]);
    let mut tall = String::from("| a |\n|---|\n");
    for n in 0..59 {
        tall.push_str(&format!("| {n} |\n"));
    }
    assert!(problems(&tall).is_empty(), "60 rows with the header are the most");
    tall.push_str("| one more |\n");
    assert_eq!(problems(&tall), [(62, Code::TableTooTall)]);
}

#[test]
fn a_table_too_wide_or_too_tall_is_cut_to_the_cap_at_run_time() {
    let wide = "| 1 | 2 | 3 | 4 | 5 | 6 | 7 |\n|---|---|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 | 6 | 7 |\n";
    let got = read(wide).topic.doc.blocks;
    let Block::Table { header, rows } = &got[0] else { panic!("{got:?}") };
    assert_eq!((header.len(), rows[0].len()), (6, 6));
}

#[test]
fn a_picture_stands_alone_with_alt_text_and_a_site_name() {
    let got = blocks("![The Snap & Glue dialog](site:snap-glue-dialog)\n");
    assert_eq!(got, [Block::Picture { alt: "The Snap & Glue dialog".to_owned(), name: "snap-glue-dialog".to_owned(), line: 8 }]);
    let escaped = blocks("![A \\[bracket\\] shown](site:b)\n");
    assert_eq!(escaped, [Block::Picture { alt: "A [bracket] shown".to_owned(), name: "b".to_owned(), line: 8 }]);
}

#[test]
fn a_picture_the_format_refuses_is_found_with_its_line() {
    assert_eq!(problems("![](site:x)\n"), [(1, Code::PictureAlt)]);
    assert_eq!(problems("![a](https://diacraft.dev/x.png)\n"), [(1, Code::PictureSyntax)]);
    assert_eq!(problems("![a](site:Bad Name)\n"), [(1, Code::PictureName)]);
    assert_eq!(problems("![a](site:)\n"), [(1, Code::PictureName)]);
    assert!(problems("text ![a](site:x) more\n").contains(&(1, Code::PictureAlone)));
}

#[test]
fn html_setext_headings_thematic_breaks_and_indented_code_are_refused_with_their_lines() {
    assert_eq!(problems("fine\n\n<b>bold</b>\n"), [(3, Code::Html)]);
    assert_eq!(problems("a <br> b\n"), [(1, Code::Html)]);
    assert_eq!(problems("a </p> b\n"), [(1, Code::Html)]);
    assert_eq!(problems("<!-- c -->\n"), [(1, Code::Html)]);
    assert_eq!(problems("Title\n=====\n"), [(2, Code::Setext)]);
    assert_eq!(problems("Title\n-----\n"), [(2, Code::Setext)]);
    assert_eq!(problems("para\n\n---\n"), [(3, Code::ThematicBreak)]);
    assert_eq!(problems("***\n"), [(1, Code::ThematicBreak)]);
    assert_eq!(problems("___\n"), [(1, Code::ThematicBreak)]);
    assert_eq!(problems("para\n\n    code\n"), [(3, Code::IndentedLine)]);
    assert_eq!(problems("\ttabbed\n"), [(1, Code::IndentedLine)]);
    assert_eq!(problems(" one space\n"), [(1, Code::IndentedLine)]);
}

#[test]
fn text_that_only_looks_like_one_of_them_is_text() {
    assert!(problems("a < b and c > d\n").is_empty());
    assert!(problems("1 <3 2\n").is_empty());
    assert_eq!(problems("- - -\n"), [(1, Code::ThematicBreak)]);
    assert!(problems("It costs 5 - 3\n").is_empty());
    assert!(problems("Use Alt+F9 -- not F9\n").is_empty());
}

#[test]
fn a_control_character_is_refused() {
    assert_eq!(problems("a\u{1}b\n"), [(1, Code::ControlCharacter)]);
    assert_eq!(problems("a\u{7f}b\n"), [(1, Code::ControlCharacter)]);
}

#[test]
fn crlf_line_ends_read_the_same() {
    let lf = read("## Heading\n\ntext one\ntext two\n");
    let crlf = read("## Heading\r\n\r\ntext one\r\ntext two\r\n");
    assert_eq!(lf.topic.doc, crlf.topic.doc);
    assert!(crlf.problems.is_empty(), "{:?}", crlf.problems);
}

#[test]
fn a_blank_body_is_an_empty_doc() {
    let parsed = read("");
    assert!(parsed.topic.doc.blocks.is_empty() && parsed.problems.is_empty());
    assert!(read("\n\n   \n").topic.doc.blocks.is_empty());
}

#[test]
fn every_text_knows_the_line_it_starts_at() {
    let got = read("## Head\n\npara one\npara two\n\n- item a\n  - nested\n\n> note\n\n| h |\n|---|\n| c |\n").topic.doc.blocks;
    let lines: Vec<usize> = got
        .iter()
        .flat_map(|b| match b {
            Block::Heading { text, .. } | Block::Para(text) | Block::Note(text) => vec![text.line],
            Block::List(list) => list
                .items
                .iter()
                .flat_map(|i| std::iter::once(i.text.line).chain(i.sub.iter().flat_map(|s| s.items.iter().map(|n| n.text.line))))
                .collect(),
            Block::Table { header, rows } => header.iter().chain(rows.iter().flatten()).map(|c| c.line).collect(),
            _ => vec![],
        })
        .collect();
    assert_eq!(lines, [8, 10, 13, 14, 16, 18, 20]);
}

#[test]
fn a_topic_lists_its_links_with_their_lines_and_its_anchors_in_order() {
    let parsed = read("## One {#first}\n\nSee [a](topic:x) and\n[b](ctl:Y).\n\n- item [c](cmd:p.q)\n\n## Two\n\n| h |\n|---|\n| [d](#first) |\n");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let links: Vec<(usize, String)> = parsed.topic.links().into_iter().map(|(line, target)| (line, format!("{target:?}"))).collect();
    assert_eq!(links.len(), 4);
    assert_eq!(links.iter().map(|l| l.0).collect::<Vec<_>>(), [10, 10, 13, 19]);
    assert!(
        links[0].1.starts_with("Topic") && links[1].1.starts_with("Control") && links[2].1.starts_with("Command") && links[3].1.starts_with("Anchor")
    );
    assert_eq!(parsed.topic.anchors(), ["first", "two"]);
}
