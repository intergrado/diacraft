//! The inlines of Help Markdown and the link targets (design section 2.3): each one parsed, and each refusal with its line.

use diacraft_help::md::{parse_inlines, slug};
use diacraft_help::{Block, Code, Span, Target};

use crate::{problems, read};

fn spans(src: &str) -> Vec<Span> {
    let parsed = read(src);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    match parsed.topic.doc.blocks.into_iter().next() {
        Some(Block::Para(t)) => t.spans,
        other => panic!("{other:?}"),
    }
}

fn t(s: &str) -> Span {
    Span::Text(s.to_owned())
}

fn link(src: &str) -> Target {
    match spans(src).into_iter().next() {
        Some(Span::Link { target, .. }) => target,
        other => panic!("{other:?}"),
    }
}

#[test]
fn strong_emphasis_and_code_are_read() {
    assert_eq!(
        spans("plain *em* and **strong** and `code` end"),
        [t("plain "), Span::Em(vec![t("em")]), t(" and "), Span::Strong(vec![t("strong")]), t(" and "), Span::Code("code".to_owned()), t(" end")]
    );
}

#[test]
fn emphasis_nests_once_either_way_round() {
    assert_eq!(spans("**a *b* c**"), [Span::Strong(vec![t("a "), Span::Em(vec![t("b")]), t(" c")])]);
    assert_eq!(spans("*a **b** c*"), [Span::Em(vec![t("a "), Span::Strong(vec![t("b")]), t(" c")])]);
    assert_eq!(spans("***x***"), [Span::Strong(vec![Span::Em(vec![t("x")])])]);
    assert_eq!(spans("*a **b***"), [Span::Em(vec![t("a "), Span::Strong(vec![t("b")])])]);
    assert_eq!(spans("**a *b***"), [Span::Strong(vec![t("a "), Span::Em(vec![t("b")])])]);
}

#[test]
fn a_code_span_is_verbatim() {
    assert_eq!(spans("`a*b [c](d) \\n`"), [Span::Code("a*b [c](d) \\n".to_owned())]);
}

#[test]
fn the_escapes_give_the_character_and_nothing_else() {
    assert_eq!(spans("\\*not em\\* \\[x\\]\\(y\\) \\` \\\\ \\|"), [t("*not em* [x](y) ` \\ |")]);
}

#[test]
fn a_link_has_text_and_a_target_of_one_of_the_kinds() {
    assert_eq!(link("[Page setup](topic:page-setup)"), Target::Topic { id: "page-setup".to_owned(), anchor: None });
    assert_eq!(link("[Margins](topic:page-setup#margins)"), Target::Topic { id: "page-setup".to_owned(), anchor: Some("margins".to_owned()) });
    assert_eq!(link("[Fit](#fit-page)"), Target::Anchor("fit-page".to_owned()));
    assert_eq!(link("[Snap](ctl:SnapAndGlueDialog)"), Target::Control("SnapAndGlueDialog".to_owned()));
    assert_eq!(link("[Gallery](ctl:_3DRotationGallery)"), Target::Control("_3DRotationGallery".to_owned()));
    assert_eq!(link("[Fit](cmd:view.fitPage)"), Target::Command("view.fitPage".to_owned()));
    match link("[Undo](key:Cmd+Z)") {
        Target::Key(chord) => assert_eq!((chord.cmd, chord.shift, chord.key.as_str()), (true, false, "Z")),
        other => panic!("{other:?}"),
    }
    assert_eq!(link("[Site](https://diacraft.dev/guides/getting-started)"), Target::Web("https://diacraft.dev/guides/getting-started".to_owned()));
    assert_eq!(link("[App](https://diacraft.app/)"), Target::Web("https://diacraft.app/".to_owned()));
    assert_eq!(
        link("[Issues](https://github.com/intergrado/diacraft/issues)"),
        Target::Web("https://github.com/intergrado/diacraft/issues".to_owned())
    );
}

#[test]
fn a_link_text_holds_emphasis_and_code_and_a_target_holds_escaped_parentheses() {
    let got = spans("[**bold** and `code`](topic:a)");
    match &got[0] {
        Span::Link { text, .. } => assert_eq!(text, &[Span::Strong(vec![t("bold")]), t(" and "), Span::Code("code".to_owned())]),
        other => panic!("{other:?}"),
    }
    assert_eq!(link("[x](https://diacraft.dev/a_\\(b\\))"), Target::Web("https://diacraft.dev/a_(b)".to_owned()));
}

#[test]
fn a_link_target_the_format_refuses_is_found_with_its_line() {
    assert_eq!(problems("fine\n\n[x](topic:)\n"), [(3, Code::TargetId)]);
    assert_eq!(problems("[x](topic:Bad_Id)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](topic:a#Bad)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](#)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](ctl:bad-id)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](cmd:nodots)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](cmd:a..b)\n"), [(1, Code::TargetId)]);
    assert_eq!(problems("[x](key:Hyper+Z)\n"), [(1, Code::TargetKey)]);
    assert_eq!(problems("[x](key:Cmd+Ctrl+Z)\n"), [(1, Code::TargetKey)]);
    for bad in [
        "http://diacraft.dev/",
        "https://example.org/",
        "https://diacraft.dev.evil/",
        "https://x@diacraft.dev/",
        "https://diacraft.dev@evil.com/",
        "https://diacraft.dev:8080/",
        "https://github.com/someone/else",
    ] {
        assert_eq!(problems(&format!("[x]({bad})\n")), [(1, Code::TargetWeb)], "{bad}");
    }
    for bad in ["javascript:alert(1)", "mailto:a@b.c", "file:///etc/passwd", "data:text/html,x", "page.html", "/abs/path", "ftp://x/"] {
        assert_eq!(problems(&format!("[x]({bad})\n")), [(1, Code::TargetSyntax)], "{bad}");
    }
}

#[test]
fn a_link_that_leaves_its_site_by_dot_segments_is_refused_like_any_other_address() {
    for bad in
        ["https://diacraft.dev/guides/../../evil", "https://diacraft.dev/guides/%2e%2e/evil", "https://github.com/intergrado/diacraft/../other"]
    {
        assert_eq!(problems(&format!("[x]({bad})\n")), [(1, Code::TargetWeb)], "{bad}");
    }
}

#[test]
fn a_link_the_format_refuses_is_shown_as_its_text() {
    let parsed = read("see [the page](https://example.org/) now\n");
    let Some(Block::Para(t)) = parsed.topic.doc.blocks.first() else { panic!() };
    assert_eq!(t.spans, [self::t("see the page now")]);
}

#[test]
fn a_target_of_512_bytes_is_the_longest() {
    let at = format!("https://diacraft.dev/{}", "a".repeat(512 - "https://diacraft.dev/".len()));
    assert_eq!(link(&format!("[x]({at})")), Target::Web(at.clone()));
    let over = format!("{at}a");
    assert_eq!(problems(&format!("[x]({over})\n")), [(1, Code::TargetTooLong)]);
    let parsed = read(&format!("[x]({over})\n"));
    let Some(Block::Para(text)) = parsed.topic.doc.blocks.first() else { panic!() };
    assert_eq!(text.spans, [t("x")], "the link is shown as text");
}

#[test]
fn delimiters_left_open_and_brackets_that_make_no_link_are_refused() {
    assert_eq!(problems("fine\n\nan *open one\n"), [(3, Code::UnclosedEmphasis)]);
    assert_eq!(problems("an **open one\n"), [(1, Code::UnclosedEmphasis)]);
    assert_eq!(problems("a `open one\n"), [(1, Code::UnclosedCode)]);
    assert_eq!(problems("[x]\n"), [(1, Code::UnmatchedBracket)]);
    assert_eq!(problems("x]\n"), [(1, Code::UnmatchedBracket)]);
    assert_eq!(problems("[x] (topic:a)\n"), [(1, Code::UnmatchedBracket)]);
    assert_eq!(problems("[x](topic:a\n"), [(1, Code::UnmatchedBracket)]);
    assert_eq!(problems("[^1]\n"), [(1, Code::UnmatchedBracket)]);
    assert_eq!(problems("[[x](topic:a)](topic:b)\n"), [(1, Code::LinkInLink)]);
}

#[test]
fn what_is_refused_stays_in_the_text_as_it_was_written() {
    let parsed = read("an *open one and [x] too\n");
    let Some(Block::Para(text)) = parsed.topic.doc.blocks.first() else { panic!() };
    assert_eq!(text.spans, [t("an *open one and [x] too")]);
}

#[test]
fn empty_spans_nesting_too_deep_star_runs_and_unknown_escapes_are_refused() {
    assert_eq!(problems("a ** ** b\n"), [(1, Code::EmptySpan)]);
    assert_eq!(problems("a `` b\n"), [(1, Code::EmptySpan)]);
    assert_eq!(problems("[](topic:a)\n"), [(1, Code::EmptySpan)]);
    assert_eq!(problems("*a **b *c* d** e*\n"), [(1, Code::EmphasisNesting)]);
    assert_eq!(problems("****x****\n"), [(1, Code::StarRun)]);
    assert_eq!(problems("a \\q b\n"), [(1, Code::UnknownEscape)]);
    assert_eq!(problems("ends with a backslash \\\n"), [(1, Code::UnknownEscape)]);
}

#[test]
fn text_with_the_marks_of_other_notations_is_plain() {
    assert_eq!(spans("snake_case_name and a_b_c, 2 \\* 3"), [t("snake_case_name and a_b_c, 2 * 3")]);
}

#[test]
fn parse_inlines_reads_a_source_on_its_own_and_reports_its_line() {
    let mut found = Vec::new();
    let got = parse_inlines("a *b", 42, &mut found);
    assert_eq!(got, [t("a *b")]);
    assert_eq!(found.iter().map(|p| (p.line, p.code)).collect::<Vec<_>>(), [(42, Code::UnclosedEmphasis)]);
    let mut clean = Vec::new();
    assert_eq!(parse_inlines("**ok**", 1, &mut clean), [Span::Strong(vec![t("ok")])]);
    assert!(clean.is_empty());
}

#[test]
fn a_slug_is_lowercase_ascii_words_with_single_dashes_and_at_most_64_bytes() {
    assert_eq!(slug("Hello World"), "hello-world");
    assert_eq!(slug("  Snap & Glue!  "), "snap-glue");
    assert_eq!(slug("Page 2 of 3"), "page-2-of-3");
    assert_eq!(slug("Café au lait"), "caf-au-lait");
    assert_eq!(slug("--a--b--"), "a-b");
    assert_eq!(slug("…"), "");
    assert_eq!(slug("日本語"), "");
    let long = "word ".repeat(40);
    let s = slug(&long);
    assert!(s.len() <= 64 && !s.ends_with('-'), "{s}");
    assert_eq!(slug(&s), s, "a slug is its own slug");
}
