//! Strings, colours, the absence of an error literal, and the GUID token: .

use diacraft_formula::{Expr, ParseErrorKind, parse};

use crate::{err, ok, shape};

fn text(src: &str) -> String {
    match ok(src) {
        Expr::Str(s) => s.to_string(),
        other => panic!("{src}: {other:?}"),
    }
}

#[test]
fn a_string_between_double_quotes() {
    assert_eq!(text("\"plain\""), "plain");
    assert_eq!(text("\"\""), "");
    assert_eq!(text("\"  spaces  \""), "  spaces  ");
    // Any text: brackets, commas, operators, sheet marks, a quote of the other kind, non-ASCII, a line break.
    assert_eq!(text("\"a,b (c) + d ! e 'f' [g] #h {i}\""), "a,b (c) + d ! e 'f' [g] #h {i}");
    assert_eq!(text("\"é ü 日本語 🙂\""), "é ü 日本語 🙂");
    assert_eq!(text("\"two\nlines\""), "two\nlines");
    assert_eq!(text("\"a\u{0}b\""), "a\u{0}b");
    // The text of a string is not read: a formula inside it stays text (reads it at evaluation).
    assert_eq!(text("\"Width*2\""), "Width*2");
    assert_eq!(text("\"2FT 3IN\""), "2FT 3IN");
    assert_eq!(shape("\"7\"*6"), "(* \"7\" 6)");
}

#[test]
fn two_double_quotes_in_a_string_are_one() {
    // E2 (community report): another reader reads `""` inside a string as a quote, and MS-VSDX says nothing against it. An empty
    // string stays empty, and `""""` is a string of one quote.
    assert_eq!(text("\"say \"\"hi\"\"\""), "say \"hi\"");
    assert_eq!(text("\"\"\"\""), "\"");
    assert_eq!(text("\"a\"\"b\""), "a\"b");
    assert_eq!(text("\"\"\"a\""), "\"a");
    assert_eq!(shape("F(\"\", \"\")"), "(call F \"\" \"\")");
    assert_eq!(shape("\"\"&\"x\""), "(& \"\" \"x\")");
    // Without the doubling, a string and then another is two operands with nothing between them.
    assert_eq!(err("\"a\" \"b\"").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn a_string_that_is_not_closed_is_an_error() {
    for src in ["\"", "\"abc", "\"abc\"\"", "\"\"\"", "F(\"abc)", "1 & \"x"] {
        let e = err(src);
        assert_eq!(e.kind, ParseErrorKind::UnterminatedString, "{src}");
    }
    assert_eq!(err("F(\"abc)").offset, 2, "the offset is where the string began");
}

#[test]
fn a_colour_is_a_hash_and_six_hex_digits() {
    assert_eq!(ok("#FF8000"), Expr::Color { r: 255, g: 128, b: 0 });
    assert_eq!(ok("#ff8000"), Expr::Color { r: 255, g: 128, b: 0 });
    assert_eq!(ok("#000000"), Expr::Color { r: 0, g: 0, b: 0 });
    assert_eq!(ok("#0a1B2c"), Expr::Color { r: 10, g: 27, b: 44 });
    assert_eq!(shape("IF(#FF8000,5,7)"), "(call IF #FF8000 5 7)");
    assert_eq!(shape("#FF8000+1"), "(+ #FF8000 1)");
    // Not six digits, or digits that go on, or a non-digit.
    for src in ["#", "#FF80", "#FF80000", "#FF800G", "#GG0000", "# FF8000", "#FF 8000"] {
        assert!(parse(src).ast.is_err(), "{src}");
    }
    assert_eq!(err("#FF80").kind, ParseErrorKind::BadHash);
}

#[test]
fn an_error_value_has_no_literal() {
    // An error value has no literal in the grammar and only arises from evaluation; the `literal` has no such form, and
    // says files write `REF()`. So `#REF!` is not read, in any of the spellings list.
    for src in ["#DIV/0!", "#DIV/0", "#VALUE!", "#REF!", "#NUM!", "#NUM", "#N/A", "#N/A!", "#NA!", "#DIM!", "#ref!", "#NAME?"] {
        assert_eq!(err(src).kind, ParseErrorKind::BadHash, "{src}");
        assert_eq!(err(&format!("IF({src},1,2)")).kind, ParseErrorKind::BadHash, "{src}");
        assert_eq!(err(&format!("1+{src}")).kind, ParseErrorKind::BadHash, "{src}");
    }
    // The way to make one on purpose is a call: `REF()` and `NA()`.
    assert_eq!(shape("REF()"), "(call REF)");
    assert_eq!(shape("NA()"), "(call NA)");
    assert_eq!(shape("IF(4<2,REF(),7)"), "(call IF (< 4 2) (call REF) 7)");
    assert_eq!(shape("ISERROR(REF())"), "(call ISERROR (call REF))");
}

/// A GUID as the files write it: `{`, eight, four, four, four and twelve hexadecimal digits, `}`.
const GUID: &str = "{00000000-1111-2222-3333-444444444444}";

fn guid_argument(src: &str) -> String {
    let Expr::Call(c) = ok(src) else { panic!("{src}") };
    assert!(c.is("USE"), "{src}");
    assert_eq!(c.args.len(), 1, "{src}");
    match &c.args[0] {
        Some(Expr::Guid(g)) => g.to_string(),
        Some(Expr::Str(s)) => s.to_string(),
        other => panic!("{src}: {other:?}"),
    }
}

#[test]
fn a_braced_guid_is_use_s_one_argument_and_reads_as_the_string_it_spells() {
    // 1,497 cells hold `USE({...})` with no quotes; it reads as the string it spells, braces included.
    assert_eq!(guid_argument(&format!("USE({GUID})")), GUID);
    // The same call with quotes round the braces is the same string.
    assert_eq!(guid_argument(&format!("USE(\"{GUID}\")")), GUID);
    assert_eq!(guid_argument(&format!("USE({GUID})")), guid_argument(&format!("USE(\"{GUID}\")")));
    assert_eq!(ok(&format!("USE({GUID})")), ok(&format!("USE( {GUID} )")));
    // Either case of the digits, kept as written, and the name of the function in any case.
    let upper = "{ABCDEF01-2345-6789-ABCD-EF0123456789}";
    let lower = "{abcdef01-2345-6789-abcd-ef0123456789}";
    assert_eq!(guid_argument(&format!("Use({upper})")), upper);
    assert_eq!(guid_argument(&format!("USE({lower})")), lower);
    // Arithmetic around the call is common (`ColorSchemeIndex`): the call is the operand, the GUID is its argument.
    assert_eq!(shape(&format!("USE({GUID})+1")), format!("(+ (call USE {GUID}) 1)"));
    assert_eq!(shape(&format!("IF(1,USE({GUID}),2)")), format!("(call IF 1 (call USE {GUID}) 2)"));
    // A quoted name still works.
    assert_eq!(shape("USE(\"Some Master\")"), "(call USE \"Some Master\")");
}

#[test]
fn a_braced_guid_anywhere_else_is_a_parse_failure() {
    // Read only as USE's one argument; anywhere else the formula fails to parse, as it did.
    for src in [
        // a bare token, in arithmetic, in another function (the own `RED(...)`), in another argument slot of USE
        GUID.to_string(),
        format!("1+{GUID}"),
        format!("{GUID}+1"),
        format!("RED({GUID})"),
        format!("IF({GUID},1,2)"),
        format!("F(1,{GUID})"),
        format!("USE(1,{GUID})"),
        format!("USE({GUID},1)"),
        format!("USE(1+{GUID})"),
        format!("USE(({GUID}))"),
        format!("USE(-{GUID})"),
        format!("USE({GUID}+1)"),
        format!("USE({GUID}{GUID})"),
        format!("USE(,{GUID})"),
        format!("({GUID})"),
        format!("Sheet.1!{GUID}"),
        // the shape of a GUID: eight, four, four, four and twelve digits, and nothing but those and hyphens
        "USE({0000})".to_string(),
        "USE({})".to_string(),
        "USE({00000000-1111-2222-3333-44444444444})".to_string(),
        "USE({00000000-1111-2222-3333-4444444444444})".to_string(),
        "USE({000000001-111-2222-3333-444444444444})".to_string(),
        "USE({00000000111122223333444444444444})".to_string(),
        "USE({00000000-1111-2222-3333-44444444444g})".to_string(),
        "USE({00000000-1111-2222-3333-444444444444)".to_string(),
        "USE({ 00000000-1111-2222-3333-444444444444})".to_string(),
        "USE({00000000-1111-2222-3333-444444444444 })".to_string(),
        "USE({ab})".to_string(),
        "USE({".to_string(),
        "USE({}".to_string(),
        format!("USE({}", GUID.trim_end_matches('}')),
        format!("USE({GUID}"),
    ] {
        let e = err(&src);
        assert!(
            matches!(
                e.kind,
                ParseErrorKind::BadGuid | ParseErrorKind::ExpectedOperand | ParseErrorKind::ExpectedArgumentSeparator | ParseErrorKind::UnexpectedEnd
            ),
            "{src}: {e}"
        );
    }
    // A GUID that is the wrong shape is a bad GUID, not a bad operand.
    assert_eq!(err("USE({0000})").kind, ParseErrorKind::BadGuid);
    assert_eq!(err(&format!("RED({GUID})")).kind, ParseErrorKind::BadGuid);
    assert_eq!(err(&format!("1+{GUID}")).kind, ParseErrorKind::BadGuid);
    assert_eq!(err(&GUID[..GUID.len() - 1]).kind, ParseErrorKind::BadGuid);
}

#[test]
fn the_other_literals_beside_each_other() {
    assert_eq!(shape("F(1, \"two\", TRUE, #0000FF, 4 in, 5%)"), "(call F 1 \"two\" TRUE #0000FF 4IN (% 5))");
}
