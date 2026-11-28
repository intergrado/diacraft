//! The header of a topic (design section 2.3).

use diacraft_help::{Code, MAX_HEADER_BYTES, MAX_KEYWORDS, parse};

const BASE: &str = "id: sample\ntitle: Sample\nsummary: A sample.\nkeywords: one, two words , three\n";

fn file(lines: &str) -> String {
    format!("---\n{lines}---\nBody.\n")
}

fn codes(lines: &str) -> Vec<(usize, Code)> {
    parse(&file(lines)).unwrap().problems.iter().map(|p| (p.line, p.code)).collect()
}

fn text(n: usize) -> String {
    "x".repeat(n)
}

#[test]
fn every_key_is_read() {
    let description = text(150);
    let lines = format!(
        "{BASE}unlisted: yes\nsite: yes\npath: guides/getting-started\ndescription: {description}\nseo_title: A short title\npicture: start-picture\n"
    );
    let parsed = parse(&file(&lines)).unwrap();
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let h = parsed.topic.header;
    assert_eq!((h.id.as_str(), h.title.as_str(), h.summary.as_str()), ("sample", "Sample", "A sample."));
    assert_eq!(h.keywords, ["one", "two words", "three"]);
    assert!(h.unlisted && h.site.publish);
    assert_eq!(h.site.path.as_deref(), Some("guides/getting-started"));
    assert_eq!(h.site.description.as_deref(), Some(description.as_str()));
    assert_eq!(h.site.seo_title.as_deref(), Some("A short title"));
    assert_eq!(h.site.picture.as_deref(), Some("start-picture"));
}

#[test]
fn a_topic_is_published_unless_it_says_site_no_and_its_path_defaults_to_its_id() {
    let parsed = parse(&file(&format!("{BASE}description: {}\n", text(150)))).unwrap();
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let h = parsed.topic.header;
    assert!(h.site.publish && !h.unlisted);
    assert_eq!(h.site.path_or_default(&h.id), "help/sample");
    let guide = parse(&file(&format!("{BASE}site: no\npath: guides/x\n"))).unwrap().topic.header;
    assert!(!guide.site.publish);
    assert_eq!(guide.site.path_or_default(&guide.id), "guides/x");
}

#[test]
fn an_unknown_key_is_a_problem_with_its_line_and_is_ignored() {
    let parsed = parse(&file(&format!("{BASE}site: no\ncolour: red\n"))).unwrap();
    let found: Vec<_> = parsed.problems.iter().map(|p| (p.line, p.code, p.detail.as_str())).collect();
    assert_eq!(found, [(7, Code::HeaderUnknownKey, "colour")]);
    assert_eq!(parsed.topic.header.id, "sample", "the rest of the header is read");
    assert!(parsed.problems[0].to_string().contains("unknown header key"), "{}", parsed.problems[0]);
}

#[test]
fn a_key_written_twice_a_line_that_is_no_pair_and_an_empty_value_are_problems() {
    assert_eq!(codes(&format!("{BASE}site: no\nsite: no\n")), [(7, Code::HeaderDuplicateKey)]);
    assert_eq!(codes(&format!("{BASE}site: no\nnot a pair\n")), [(7, Code::HeaderLine)]);
    assert_eq!(codes(&format!("{BASE}site: no\npicture:\n")), [(7, Code::HeaderValue)]);
}

#[test]
fn id_and_title_are_needed_to_read_a_topic_and_summary_and_keywords_are_required() {
    let no_id = parse(&file("title: T\nsummary: S.\nkeywords: k\nsite: no\n")).unwrap_err();
    assert_eq!((no_id.code, no_id.line), (Code::HeaderMissing, 0), "{no_id}");
    let no_title = parse(&file("id: x\nsummary: S.\nkeywords: k\nsite: no\n")).unwrap_err();
    assert_eq!(no_title.code, Code::HeaderMissing);
    assert_eq!(codes("id: x\ntitle: T\nkeywords: k\nsite: no\n"), [(0, Code::HeaderMissing)]);
    assert_eq!(codes("id: x\ntitle: T\nsummary: S.\nsite: no\n"), [(0, Code::HeaderMissing)]);
}

#[test]
fn an_id_is_lowercase_words_with_dashes() {
    for ok in ["a", "page-setup", "snap-and-glue", "p2", "a-1-b"] {
        assert!(parse(&file(&format!("id: {ok}\ntitle: T\nsummary: S.\nkeywords: k\nsite: no\n"))).is_ok(), "{ok}");
    }
    for bad in ["Page", "page_setup", "-a", "a-", "a--b", "a b", "é", &text(65)] {
        let e = parse(&file(&format!("id: {bad}\ntitle: T\nsummary: S.\nkeywords: k\nsite: no\n"))).unwrap_err();
        assert_eq!(e.code, Code::HeaderValue, "{bad}: {e}");
    }
}

#[test]
fn the_lengths_of_the_site_fields_and_the_others_are_held() {
    let with = |extra: &str| codes(&format!("{BASE}{extra}"));
    // title 80, summary 160
    assert!(parse(&file(&format!("id: x\ntitle: {}\nsummary: {}\nkeywords: k\nsite: no\n", text(80), text(160)))).unwrap().problems.is_empty());
    let long = parse(&file(&format!("id: x\ntitle: {}\nsummary: {}\nkeywords: k\nsite: no\n", text(81), text(161)))).unwrap();
    assert_eq!(long.problems.iter().map(|p| (p.line, p.code)).collect::<Vec<_>>(), [(3, Code::HeaderLength), (4, Code::HeaderLength)]);
    // description 140 to 160 when published
    for n in [140, 150, 160] {
        assert!(with(&format!("description: {}\n", text(n))).is_empty(), "{n}");
    }
    for n in [1, 139, 161] {
        let found = with(&format!("description: {}\n", text(n)));
        assert_eq!(found.iter().map(|f| f.1).collect::<Vec<_>>(), [Code::HeaderLength], "{n}");
    }
    // seo_title 60
    assert!(with(&format!("site: no\nseo_title: {}\n", text(60))).is_empty());
    assert_eq!(with(&format!("site: no\nseo_title: {}\n", text(61))), [(7, Code::HeaderLength)]);
    // characters, not bytes
    assert!(with(&format!("description: {}\n", "é".repeat(150))).is_empty());
}

#[test]
fn a_published_topic_needs_its_description_and_one_that_is_not_published_does_not() {
    assert_eq!(codes(BASE), [(0, Code::HeaderMissing)]);
    assert!(codes(&format!("{BASE}site: no\n")).is_empty());
}

#[test]
fn site_and_unlisted_are_yes_or_no_and_the_picture_and_path_are_names() {
    assert_eq!(codes(&format!("{BASE}site: maybe\n")), [(6, Code::HeaderValue)]);
    assert_eq!(codes(&format!("{BASE}site: no\nunlisted: perhaps\n")), [(7, Code::HeaderValue)]);
    assert_eq!(codes(&format!("{BASE}site: no\npicture: Bad Name\n")), [(7, Code::HeaderValue)]);
    for bad in ["../x", "/abs", "a//b", "a b", "A", "a/"] {
        assert_eq!(codes(&format!("{BASE}site: no\npath: {bad}\n")), [(7, Code::HeaderValue)], "{bad}");
    }
}

#[test]
fn keywords_are_split_at_commas_and_capped() {
    let many = (0..=MAX_KEYWORDS).map(|n| format!("k{n}")).collect::<Vec<_>>().join(", ");
    let found = codes(&format!("id: x\ntitle: T\nsummary: S.\nkeywords: {many}\nsite: no\n"));
    assert_eq!(found, [(5, Code::TooManyKeywords)]);
    let parsed = parse(&file("id: x\ntitle: T\nsummary: S.\nkeywords: a,, b ,\nsite: no\n")).unwrap();
    assert_eq!(parsed.topic.header.keywords, ["a", "b"]);
}

#[test]
fn a_file_with_no_header_or_an_open_one_is_refused_with_words() {
    let none = parse("Body only.\n").unwrap_err();
    assert_eq!((none.code, none.line), (Code::NoHeader, 1), "{none}");
    let open = parse(&format!("---\n{BASE}Body.\n")).unwrap_err();
    assert_eq!(open.code, Code::HeaderNotClosed, "{open}");
    assert!(open.to_string().contains("never closed"), "{open}");
    assert_eq!(parse("").unwrap_err().code, Code::NoHeader);
}

#[test]
fn a_header_over_the_cap_is_refused_and_one_at_the_cap_is_read() {
    // The filler is the value of `picture`, which is read last and which a name may not be as long as: the length of the header is what is held first.
    let at_size = |bytes: usize| {
        let fixed = format!("---\n{BASE}site: no\npicture: \n---\n");
        let fill = bytes - fixed.len();
        format!("---\n{BASE}site: no\npicture: {}\n---\nBody.\n", "a".repeat(fill))
    };
    let at = parse(&at_size(MAX_HEADER_BYTES)).unwrap();
    assert_eq!(at.topic.header.id, "sample");
    let over = parse(&at_size(MAX_HEADER_BYTES + 1)).unwrap_err();
    assert_eq!(over.code, Code::HeaderTooLarge, "{over}");
}

#[test]
fn the_body_starts_after_the_header_and_its_lines_count_from_the_file() {
    let parsed = parse(&format!("{}\nfirst *line\n", file(&format!("{BASE}site: no\n")))).unwrap();
    assert_eq!(parsed.topic.body, "Body.\n\nfirst *line\n");
    assert_eq!(parsed.problems.iter().map(|p| (p.line, p.code)).collect::<Vec<_>>(), [(10, Code::UnclosedEmphasis)]);
}
