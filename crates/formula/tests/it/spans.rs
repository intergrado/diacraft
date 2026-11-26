//! Where each cell reference sits in the text of a formula (`reference_spans`), so that a command that numbers rows again can write the new number into
//! the references that name them and leave every other byte of the formula as it was (a formula is kept verbatim: AGENTS.md).
//!
//! A span is held to the one rule that makes it safe to rewrite by: the text it covers, read alone, is the reference the parser made of it.

use diacraft_formula::{CellRef, Expr, Index, Qualifier, parse, reference_spans};

use crate::ok;

/// The text of the source a span covers.
fn text(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap()
}

#[test]
fn every_reference_is_found_in_the_order_it_starts_with_the_bytes_it_covers() {
    let source = "Geometry1.X4+Scratch.A3 * 2";
    let spans = reference_spans(source).unwrap();
    let covered: Vec<&str> = spans.iter().map(|s| text(source, s.start, s.end)).collect();
    assert_eq!(covered, ["Geometry1.X4", "Scratch.A3"]);
    // The path's segments: where each name sits.
    let names: Vec<&str> = spans[0].segments.iter().map(|s| text(source, s.name.start, s.name.end)).collect();
    assert_eq!(names, ["Geometry1", "X4"]);
    assert!(spans[0].segments.iter().all(|s| s.index.is_none()));
    assert_eq!(spans[1].reference.path.len(), 2);
}

#[test]
fn a_qualified_reference_covers_its_qualifiers_but_its_segments_are_the_path_only() {
    let source = "  = Sheet.10!Scratch.A3 + Pages[Site 2]!Sheet.9!Width";
    let spans = reference_spans(source).unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(text(source, spans[0].start, spans[0].end), "Sheet.10!Scratch.A3");
    let path: Vec<&str> = spans[0].segments.iter().map(|s| text(source, s.name.start, s.name.end)).collect();
    assert_eq!(path, ["Scratch", "A3"], "the qualifier is not in the path");
    assert_eq!(text(source, spans[1].start, spans[1].end), "Pages[Site 2]!Sheet.9!Width");
    assert_eq!(spans[1].reference.qualifiers.len(), 2);
    assert_eq!(text(source, spans[1].segments[0].name.start, spans[1].segments[0].name.end), "Width");
}

#[test]
fn a_row_number_in_brackets_has_the_bytes_of_its_digits() {
    let source = "Char.Size[2] + Scratch.A[ 31 ] + Prop.Cost[Site]";
    let spans = reference_spans(source).unwrap();
    assert_eq!(spans.len(), 3);
    let size = &spans[0].segments[1];
    assert_eq!(text(source, size.name.start, size.name.end), "Size");
    let index = size.index.clone().unwrap();
    assert_eq!(text(source, index.start, index.end), "2");
    let a = &spans[1].segments[1];
    let index = a.index.clone().unwrap();
    assert_eq!(text(source, index.start, index.end), "31", "the white space inside the brackets is not part of it");
    // A name in brackets is no row number: the span has no digits to rewrite.
    assert!(spans[2].segments[1].index.is_none());
    assert_eq!(spans[2].reference.path[1].index, Some(Index::Name("Site".into())));
}

#[test]
fn a_quoted_name_keeps_its_quotes_in_its_span() {
    let source = "'My row'.X + 'Odd''name'!Width";
    let spans = reference_spans(source).unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(text(source, spans[0].segments[0].name.start, spans[0].segments[0].name.end), "'My row'");
    assert_eq!(text(source, spans[1].start, spans[1].end), "'Odd''name'!Width");
}

#[test]
fn strings_calls_and_booleans_are_no_references() {
    let source = "IF(TRUE, \"Scratch.A3\", Width) & LEN(\"Geometry1.X4\") + FOO ( Prop.Cost )";
    let spans = reference_spans(source).unwrap();
    let covered: Vec<&str> = spans.iter().map(|s| text(source, s.start, s.end)).collect();
    assert_eq!(covered, ["Width", "Prop.Cost"], "a reference inside a call's arguments is found, the call and its name are not");
    assert!(reference_spans("1+2").unwrap().is_empty());
    assert!(reference_spans("\"x\"").unwrap().is_empty());
}

#[test]
fn a_reference_in_the_arguments_of_a_call_that_qualifies_another_comes_after_it() {
    let source = "LISTSHEETREF(Sheet.1!X1)!Width + Height";
    let spans = reference_spans(source).unwrap();
    let covered: Vec<&str> = spans.iter().map(|s| text(source, s.start, s.end)).collect();
    // The outer reference starts first; the inner one is inside its qualifier; then the next.
    assert_eq!(covered, ["LISTSHEETREF(Sheet.1!X1)!Width", "Sheet.1!X1", "Height"]);
    assert!(matches!(spans[0].reference.qualifiers.first(), Some(Qualifier::Call { .. })));
}

#[test]
fn a_text_that_does_not_parse_gives_its_parse_error() {
    let error = reference_spans("Scratch.A3 +").unwrap_err();
    assert_eq!(error, parse("Scratch.A3 +").ast.unwrap_err());
    assert!(reference_spans("").is_err());
    assert!(reference_spans("Scratch.").is_err());
}

#[test]
fn the_text_a_span_covers_is_the_reference_the_parser_made_of_it() {
    for source in [
        "Width",
        "=Geometry1.X4+Geometry12.Y100",
        "  Sheet.4!Connections.X1 ",
        "PAR(PNT(Sheet.20!Connections.X3,Sheet.20!Connections.Y3))",
        "IF(Prop.Cost.Value > 3, Scratch.B[7], 'Odd name'.X)",
        "Pages[Site-2]!Sheet.9!EndY - ThePage!User.Total",
        "SUM(Char.Size[2], Para.IndFirst[1]) * 2 ^ Scratch.A",
        "-Scratch.A1 + (Geometry2.NoFill)",
        "TheDoc!User.Rate*2 & \"text\" & Prop.Cost.Label",
    ] {
        for span in reference_spans(source).unwrap() {
            let alone = text(source, span.start, span.end);
            let Expr::Ref(r) = ok(alone) else { panic!("{alone:?} in {source:?} is no reference") };
            assert_eq!(*r, span.reference, "{alone:?} in {source:?}");
            assert_eq!(span.segments.len(), span.reference.path.len(), "{alone:?}");
            for (segment, seen) in span.reference.path.iter().zip(&span.segments) {
                let name = text(source, seen.name.start, seen.name.end);
                assert!(name.contains(segment.name.as_ref()) || name.starts_with('\''), "{name:?} holds {:?}", segment.name);
                assert!(seen.name.start >= span.start && seen.name.end <= span.end);
                assert_eq!(seen.index.is_some(), matches!(segment.index, Some(Index::Number(_))), "{alone:?}");
            }
        }
    }
}

/// A formula of the parser's limits costs a pass over its text, and the spans are as many as the references.
#[test]
fn a_formula_of_thousands_of_references_is_read_once() {
    // The arguments of one call: a chain of `+` is held to the parser's depth, 512 terms.
    let body = format!("SUM({})", vec!["Scratch.A3"; 6_500].join(","));
    let spans = reference_spans(&body).unwrap();
    assert_eq!(spans.len(), 6_500);
    assert_eq!(spans.last().map(|s| s.end), Some(body.len() - 1));
    let cut: &CellRef = &spans[0].reference;
    assert_eq!(cut.path.len(), 2);
}

mod props {
    use diacraft_formula::{Expr, parse, reference_spans};
    use proptest::prelude::*;

    /// A text of the pieces a reference is made of and the bytes that break one: names, dots, brackets, bangs, quotes, brackets of calls, digits, blanks, operators and
    /// a character that is more than a byte.
    fn text() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            Just("Scratch".to_owned()),
            Just("Geometry3".to_owned()),
            Just("Sheet".to_owned()),
            Just("Pages".to_owned()),
            Just("X4".to_owned()),
            Just("A".to_owned()),
            Just("é".to_owned()),
            Just(".".to_owned()),
            Just("!".to_owned()),
            Just("[".to_owned()),
            Just("]".to_owned()),
            Just("'".to_owned()),
            Just("\"".to_owned()),
            Just("(".to_owned()),
            Just(")".to_owned()),
            Just(",".to_owned()),
            Just("+".to_owned()),
            Just(" ".to_owned()),
            Just("=".to_owned()),
            (0u32..40).prop_map(|n| n.to_string()),
        ];
        proptest::collection::vec(piece, 0..24).prop_map(|pieces| pieces.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 4000, ..ProptestConfig::default() })]

        /// Any text gives its spans or the parser's error (the same error `parse` gives), never a panic; every span lies inside the text on character boundaries,
        /// in the order the references begin; the text it covers read alone is the reference it holds; and its segments lie inside it, in order.
        #[test]
        fn any_text_gives_spans_that_are_inside_it_and_read_back_as_the_references(source in text()) {
            match reference_spans(&source) {
                Err(e) => prop_assert_eq!(Some(e), parse(&source).ast.err()),
                Ok(spans) => {
                    let mut last_start = 0;
                    for span in &spans {
                        prop_assert!(span.start <= span.end && span.end <= source.len());
                        prop_assert!(span.start >= last_start, "references are listed as they begin");
                        last_start = span.start;
                        let alone = source.get(span.start..span.end);
                        prop_assert!(alone.is_some(), "on character boundaries");
                        let again = parse(alone.unwrap_or_default());
                        prop_assert_eq!(again.ast.ok(), Some(Expr::Ref(Box::new(span.reference.clone()))));
                        prop_assert_eq!(span.segments.len(), span.reference.path.len());
                        let mut at = span.start;
                        for segment in &span.segments {
                            prop_assert!(segment.name.start >= at && segment.name.start <= segment.name.end && segment.name.end <= span.end);
                            prop_assert!(source.get(segment.name.clone()).is_some());
                            at = segment.name.end;
                            if let Some(digits) = &segment.index {
                                prop_assert!(digits.start >= at && digits.end <= span.end);
                                prop_assert!(source.get(digits.clone()).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())));
                                at = digits.end;
                            }
                        }
                    }
                }
            }
        }
    }
}
