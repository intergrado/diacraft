//! Where the index of a `Pages[...]` qualifier sits in a formula's text (`page_index_spans`), for a program that rewrites the name a formula gives a page and must leave
//! every other byte as it was. A reference to a page is `Pages[<page>]!`; the bracket holds a name, a whole number or a quoted name.

use diacraft_formula::{Index, MAX_SOURCE_BYTES, page_index_spans, parse};

fn name(text: &str) -> Index {
    Index::Name(text.into())
}

/// The text each index of `source` covers and what it names.
fn found(source: &str) -> Vec<(String, Index)> {
    page_index_spans(source).into_iter().map(|s| (source.get(s.inside).unwrap().to_owned(), s.index)).collect()
}

#[test]
fn a_name_a_number_and_a_quoted_name_are_found_with_the_bytes_between_the_brackets() {
    assert_eq!(found("Pages[page1]!Sheet.9!EndY"), [("page1".to_owned(), name("page1"))]);
    assert_eq!(found("Pages[2]!Sheet.9!EndY"), [("2".to_owned(), Index::Number(2))]);
    // The bytes between the brackets include the blanks and the quotes, so that a rewrite puts a name where all of them were; the name is without them.
    assert_eq!(found("Pages[ Site 2 ]!Sheet.9!EndY"), [(" Site 2 ".to_owned(), name("Site 2"))]);
    assert_eq!(found("Pages['It''s']!Sheet.9!EndY"), [("'It''s'".to_owned(), name("It's"))]);
    assert_eq!(found("Pages['2']!Sheet.9!EndY"), [("'2'".to_owned(), name("2"))]);
    // A number too large for one is a name, as the parser reads it.
    assert_eq!(found("Pages[99999999999]!Sheet.9!EndY"), [("99999999999".to_owned(), name("99999999999"))]);
}

#[test]
fn the_spans_are_where_the_parser_reads_the_same_index() {
    for source in ["Pages[page1]!Sheet.9!EndY", "Pages[2]!Sheet.9!EndY", "Pages['It''s']!Sheet.9!EndY", "Pages[Site-2]!ThePage!PAGENAME()"] {
        let parsed = parse(source);
        let tree = format!("{:?}", parsed.ast.as_ref().unwrap());
        let spans = page_index_spans(source);
        assert_eq!(spans.len(), 1, "{source}");
        let index = format!("{:?}", spans[0].index);
        assert!(tree.contains(&index), "{source}: the tree {tree} holds {index}");
    }
}

#[test]
fn every_qualifier_of_a_formula_is_found_in_the_order_of_the_text_and_in_any_case_of_the_word() {
    let source = "Pages[A]!Sheet.1!Width+IF(pages[B]!Sheet.2!Width>1,PAGES[3]!Sheet.4!Width,0)";
    let all = found(source);
    assert_eq!(all, [("A".to_owned(), name("A")), ("B".to_owned(), name("B")), ("3".to_owned(), Index::Number(3))]);
    // A qualifier that sits behind another is found as well.
    assert_eq!(found("ThePage!Pages[A]!Sheet.1!Width").len(), 1);
}

#[test]
fn what_is_not_a_page_qualifier_is_not_found() {
    for source in [
        // A string, a quoted name, and the word inside a longer name or a path.
        r#"IF(1,"Pages[A]!Sheet.1!Width",0)"#,
        "'Pages[A]'!Width",
        "MyPages[A]!Width",
        "Prop.Pages[2]",
        "Sheet.5!Prop.Pages[1]",
        "_Pages[A]",
        "Pages2[A]",
        // No index, an empty index, a bracket that is not closed, a quoted name that is not closed.
        "Pages!Width",
        "Pages[]!Width",
        "Pages[   ]!Width",
        "Pages[A!Width",
        "Pages['A!Width",
        "Pages['A'",
        "Pages [A]!Width",
        "",
    ] {
        assert_eq!(found(source), [], "{source}");
    }
}

#[test]
fn text_after_a_string_or_a_quoted_name_is_looked_at_again() {
    assert_eq!(found(r#"IF("a"="b",Pages[A]!Sheet.1!Width,'Pages[B]'!Width)"#), [("A".to_owned(), name("A"))]);
    // A doubled quote in a string, and a doubled apostrophe in a quoted name, do not end them early.
    assert_eq!(found(r#""say ""Pages[A]"" now"&Pages[B]!Sheet.1!X"#), [("B".to_owned(), name("B"))]);
    assert_eq!(found("'It''s Pages[A]'!X+Pages[C]!Sheet.1!X"), [("C".to_owned(), name("C"))]);
}

#[test]
fn an_index_longer_than_the_parser_reads_is_not_found_and_hostile_text_is_bounded() {
    // The parser reads at most 256 bytes of an unquoted index.
    let long = format!("Pages[{}]!X", "a".repeat(300));
    assert_eq!(found(&long), []);
    let edge = format!("Pages[{}]!X", "a".repeat(256));
    assert_eq!(found(&edge).len(), 1);
    // A quoted index may be as long as a name, 1,024 bytes.
    assert_eq!(found(&format!("Pages['{}']!X", "a".repeat(1024))).len(), 1);
    assert_eq!(found(&format!("Pages['{}']!X", "a".repeat(1025))), []);
    assert!(parse(&format!("Pages['{}']!X", "a".repeat(1025))).ast.is_err(), "the parser reads no longer one either");
    // A megabyte of qualifiers, a megabyte of brackets and of quotes: one pass, no more found than the text can hold.
    let many = "Pages[A]!".repeat(MAX_SOURCE_BYTES / 9);
    assert_eq!(page_index_spans(&many).len(), MAX_SOURCE_BYTES / 9);
    for hostile in
        ["Pages[".repeat(MAX_SOURCE_BYTES / 6), "'".repeat(MAX_SOURCE_BYTES), "Pages['".repeat(MAX_SOURCE_BYTES / 7), "\"".repeat(MAX_SOURCE_BYTES)]
    {
        assert!(page_index_spans(&hostile).len() <= hostile.len());
    }
}
