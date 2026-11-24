//! The printer: text that parses back to the same tree, with the brackets the ranks of R12 need and no more.

use std::sync::Arc;

use diacraft_formula::{BinaryOp, CellRef, Expr, Index, MAX_DEPTH, MAX_SOURCE_BYTES, Number, Qualifier, Segment, UnaryOp, parse, print};
use diacraft_units::Unit;

use crate::{bin, n, neg, ok, within};

/// The text `print` gives for `src`, which must parse.
fn printed(src: &str) -> String {
    print(&ok(src))
}

/// Parses, prints, parses again, and checks that the trees are one.
fn round(src: &str) -> String {
    let tree = ok(src);
    let text = print(&tree);
    match parse(&text).ast {
        Ok(again) => assert_eq!(tree, again, "{src:?} printed as {text:?}"),
        Err(e) => panic!("{src:?} printed as {text:?}, which does not parse: {e}"),
    }
    text
}

#[test]
fn simple_forms_print_in_a_fixed_shape() {
    for (src, text) in [
        ("1+2", "1 + 2"),
        ("=1+2", "1 + 2"),
        ("  Width*2  ", "Width * 2"),
        ("IF(a>1,2,3)", "IF(a > 1, 2, 3)"),
        ("F(1,,3)", "F(1, , 3)"),
        ("F()", "F()"),
        ("BOUND(,0,FALSE,1,2)", "BOUND(, 0, FALSE, 1, 2)"),
        ("true", "TRUE"),
        ("\"a\"\"b\"", "\"a\"\"b\""),
        ("#ff8000", "#FF8000"),
        ("5 GT 2", "5 _GT_ 2"),
        ("5 _GT_ 2", "5 _GT_ 2"),
        ("a<>b", "a <> b"),
        ("-x", "-x"),
        ("- - x", "--x"),
        ("40%", "40%"),
        ("Sheet.12!PinX", "Sheet.12!PinX"),
        ("Pages[Site-2]!Sheet.9!EndY", "Pages[Site-2]!Sheet.9!EndY"),
        ("'Left-Door'!Width", "'Left-Door'!Width"),
        ("'O''Brien'!Width", "'O''Brien'!Width"),
        ("LISTSHEETREF(2)!Width", "LISTSHEETREF(2)!Width"),
        ("Sheet.5!SheetRef()", "Sheet.5!SheetRef()"),
        ("Char.Size[2]", "Char.Size[2]"),
        ("REF()", "REF()"),
        // Changed from `"12.5 USD"`: no blank before the code, as `V` holds a currency. A reader takes either.
        ("12.5USD", "12.5USD"),
        ("1 in", "1 IN"),
        ("3IN^2", "3 IN^2"),
        ("3[cm,d]", "3[CM,D]"),
        ("8FT 4IN", "8 FT 4 IN"),
        ("10°15'30\"", "10°15'30\""),
        ("10°", "10°"),
        ("10°15'", "10°15'"),
        ("2C4", "2C4"),
        ("3P6", "3P6"),
        ("5 1/2", "5.5"),
        ("1.5E3", "1500"),
        (".5", "0.5"),
        // A number is written in full from 1e-5 up to under 1e16, and with an exponent beyond, where the digits would run on.
        ("1e-4", "0.0001"),
        ("0.00001", "0.00001"),
        ("1e-7", "1e-7"),
        ("1e15", "1000000000000000"),
        ("1e16", "1e16"),
        ("1.5e300", "1.5e300"),
        ("123456789012345678", "1.2345678901234568e17"),
        ("USE({4E6E1E0E-1B4F-4D5B-9A8B-0123456789AB})", "USE({4E6E1E0E-1B4F-4D5B-9A8B-0123456789AB})"),
    ] {
        assert_eq!(printed(src), text, "{src:?}");
        round(src);
    }
}

#[test]
fn brackets_appear_where_the_ranks_need_them_and_not_elsewhere() {
    for (src, text) in [
        // The inner operator ranks lower than the outer one: brackets.
        ("(1+2)*3", "(1 + 2) * 3"),
        ("1+2*3", "1 + 2 * 3"),
        ("(1+2)^2", "(1 + 2) ^ 2"),
        ("(1&2)=3", "1 & 2 = 3"),
        ("1&(2=3)", "1 & (2 = 3)"),
        // Equal ranks run from the left: the right operand of one rank needs brackets, the left does not.
        ("1-2-3", "1 - 2 - 3"),
        ("1-(2-3)", "1 - (2 - 3)"),
        ("2^3^2", "2 ^ 3 ^ 2"),
        ("2^(3^2)", "2 ^ (3 ^ 2)"),
        ("1/(2/3)", "1 / (2 / 3)"),
        ("(1<2)<3", "1 < 2 < 3"),
        ("1<(2<3)", "1 < (2 < 3)"),
        // The sign applies to the base of a power: `-4^2` is (-4)^2 and needs none, `-(4^2)` needs them.
        ("-4^2", "-4 ^ 2"),
        ("(-4)^2", "-4 ^ 2"),
        ("-(4^2)", "-(4 ^ 2)"),
        ("2^-1", "2 ^ -1"),
        ("2^(-1)", "2 ^ -1"),
        ("-(1+2)", "-(1 + 2)"),
        ("-x*3", "-x * 3"),
        ("-(x*3)", "-(x * 3)"),
        ("1--2", "1 - -2"),
        ("1-(-2)", "1 - -2"),
        // A percent takes an atom, and a sign is above it.
        ("(1+2)%", "(1 + 2)%"),
        ("(-3)%", "(-3)%"),
        ("-3%", "-3%"),
        ("-(3%)", "-3%"),
        ("200%%", "200%%"),
        ("2^50%", "2 ^ 50%"),
        ("(2^50)%", "(2 ^ 50)%"),
        ("Width%", "Width%"),
        ("F(1)%", "F(1)%"),
        // The operator `^` on a unit-bearing literal is printed with its blanks, which is how it reads apart from the literal's own
        // power (a power straight after the unit). No bracket is added: it would be one more level of nesting than the text had.
        ("(3 in)^2", "3 IN ^ 2"),
        ("3 IN^2", "3 IN^2"),
        ("(3 in)^x", "3 IN ^ x"),
        ("(3 IN^2)^2", "3 IN^2 ^ 2"),
        ("2 cm^1.5", "2 CM ^ 1.5"),
        ("2^3 in", "2 ^ 3 IN"),
        // Brackets around an operand that needs none are dropped.
        ("((1))", "1"),
        ("(((a)))+(b)", "a + b"),
        ("F((1+2))", "F(1 + 2)"),
    ] {
        assert_eq!(printed(src), text, "{src:?}");
        round(src);
    }
}

#[test]
fn a_name_that_would_read_as_something_else_is_quoted() {
    // The words, a name with a character outside letters, digits and underscores, an empty name, one starting with a digit.
    let single = |name: &str| Expr::Ref(Box::new(CellRef { qualifiers: vec![], path: vec![Segment::new(name)] }));
    for (name, text) in [
        ("TRUE", "'TRUE'"),
        ("false", "'false'"),
        ("Width", "Width"),
        ("TRUEX", "TRUEX"),
        ("Left-Door", "'Left-Door'"),
        ("O'Brien", "'O''Brien'"),
        ("", "''"),
        ("1abc", "'1abc'"),
        ("a b", "'a b'"),
        ("_x1", "_x1"),
        ("é", "é"),
    ] {
        let tree = single(name);
        assert_eq!(print(&tree), text, "{name:?}");
        // The empty name does not read back: `''` is an error (a quoted name is not empty). Everything else does.
        if !name.is_empty() {
            assert_eq!(parse(text).ast, Ok(tree), "{name:?}");
        }
    }
    // The words are only words when nothing qualifies them, and only a whole path of one name.
    let path = |names: &[&str]| Expr::Ref(Box::new(CellRef { qualifiers: vec![], path: names.iter().map(|s| Segment::new(s)).collect() }));
    assert_eq!(print(&path(&["TRUE", "x"])), "TRUE.x");
    assert_eq!(parse("TRUE.x").ast, Ok(path(&["TRUE", "x"])));
    // After the first, a name of digits is not quoted (`Sheet.12`), and one with a space is.
    assert_eq!(print(&path(&["Sheet", "12"])), "Sheet.12");
    assert_eq!(print(&path(&["Sheet", "1 2"])), "Sheet.'1 2'");
    assert_eq!(print(&path(&["Sheet", ""])), "Sheet.''");
}

#[test]
fn an_index_is_printed_so_it_reads_back_as_itself() {
    let with_index = |index: Index| Expr::Ref(Box::new(CellRef { qualifiers: vec![], path: vec![Segment { name: "P".into(), index: Some(index) }] }));
    for (index, text) in [
        (Index::Number(2), "P[2]"),
        (Index::Number(0), "P[0]"),
        (Index::Name("Site-2".into()), "P[Site-2]"),
        (Index::Name("2".into()), "P['2']"),
        (Index::Name("a]b".into()), "P['a]b']"),
        (Index::Name("O'B".into()), "P['O''B']"),
        (Index::Name(" lead".into()), "P[' lead']"),
        (Index::Name("trail ".into()), "P['trail ']"),
        (Index::Name("Site 2".into()), "P[Site 2]"),
        (Index::Name("99999999999".into()), "P['99999999999']"),
    ] {
        let tree = with_index(index);
        assert_eq!(print(&tree), text);
        assert_eq!(parse(text).ast, Ok(tree), "{text}");
    }
}

#[test]
fn a_qualifier_that_is_a_call_is_printed_as_one() {
    let tree = Expr::Ref(Box::new(CellRef {
        qualifiers: vec![
            Qualifier::Path(vec![Segment::new("Sheet"), Segment::new("5")]),
            Qualifier::Call { name: Arc::from("LISTSHEETREF"), args: vec![Some(n(2.0)), None] },
        ],
        path: vec![Segment::new("Width")],
    }));
    assert_eq!(print(&tree), "Sheet.5!LISTSHEETREF(2, )!Width");
    assert_eq!(parse("Sheet.5!LISTSHEETREF(2, )!Width").ast, Ok(tree));
}

#[test]
fn a_string_is_printed_with_its_quotes_doubled() {
    for text in ["", "plain", "say \"hi\"", "\"", "\"\"", "a,b", "é 日本", "line\nbreak"] {
        let tree = Expr::Str(text.into());
        let printed = print(&tree);
        assert_eq!(parse(&printed).ast, Ok(tree), "{text:?} -> {printed:?}");
    }
    assert_eq!(print(&Expr::Str("say \"hi\"".into())), "\"say \"\"hi\"\"\"");
}

#[test]
fn numbers_print_so_they_read_back_exactly() {
    for v in [0.0, 1.0, 0.5, 0.1, 0.1 + 0.2, 1e21, 1e300, 1e-7, 5e-324, 1.7976931348623157e308, 123456789.12345679, 2.0_f64.powi(60), 1.0 / 3.0] {
        let tree = n(v);
        let text = print(&tree);
        assert_eq!(parse(&text).ast, Ok(tree), "{v:e} -> {text}");
    }
    // Compound parts too: no exponent in the second part of `2C4`.
    for (tree, text) in [
        (Number::CiceroDidot { ciceros: 2.0, didots: 1e21 }, "2C1000000000000000000000"),
        (Number::PicaPoints { picas: 3.0, points: 0.25 }, "3P0.25"),
    ] {
        assert_eq!(print(&Expr::Number(tree)), text);
        assert_eq!(parse(text).ast, Ok(Expr::Number(tree)));
    }
}

#[test]
fn display_is_print() {
    let tree = ok("1+2*3");
    assert_eq!(tree.to_string(), print(&tree));
}

#[test]
fn a_hand_built_tree_prints_without_a_stack_overflow() {
    // A tree this crate did not parse may be as deep as its builder likes: print stops at MAX_DEPTH and writes `...`.
    let text = within(60, || {
        let mut e = n(1.0);
        for _ in 0..100_000 {
            e = neg(e);
        }
        let text = print(&e);
        // Dropping 100,000 levels recursively would overflow too: take the tree apart from the top.
        let mut cur = e;
        while let Expr::Unary(_, inner) = cur {
            cur = *inner;
        }
        text
    });
    assert!(text.contains("...") && text.len() < 4 * MAX_DEPTH + 16, "{}", text.len());
    // A depth a parse can give prints whole.
    let mut e = n(1.0);
    for _ in 0..(MAX_DEPTH - 1) {
        e = neg(e);
    }
    let whole = print(&e);
    assert!(!whole.contains("...") && whole.len() == MAX_DEPTH, "{} {}", whole.len(), MAX_DEPTH);
}

#[test]
fn binary_and_unary_operators_have_one_printed_form_each() {
    // Each operator's symbol, between operands: `a OP b`.
    let a = || n(1.0);
    for op in [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Pow,
        BinaryOp::Cat,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::Eq,
        BinaryOp::Ne,
        BinaryOp::ExactLt,
        BinaryOp::ExactLe,
        BinaryOp::ExactGt,
        BinaryOp::ExactGe,
        BinaryOp::ExactEq,
        BinaryOp::ExactNe,
    ] {
        let tree = bin(op, a(), a());
        assert_eq!(print(&tree), format!("1 {} 1", op.symbol()));
        assert_eq!(parse(&print(&tree)).ast, Ok(tree));
    }
    for op in [UnaryOp::Plus, UnaryOp::Minus] {
        let tree = Expr::Unary(op, Box::new(a()));
        assert_eq!(print(&tree), format!("{}1", op.symbol()));
        assert_eq!(parse(&print(&tree)).ast, Ok(tree));
    }
    // The literal with a unit, the only way a unit is printed.
    assert_eq!(print(&Expr::Number(Number::with_unit(2.0, Unit::Inch))), "2 IN");
}

/// What the text of a tree is when it is read again: the tree, or the error that stops it.
fn read_back(src: &str) -> Result<(), String> {
    let tree = ok(src);
    let text = print(&tree);
    match parse(&text).ast {
        Ok(again) if again == tree => Ok(()),
        Ok(_) => Err(format!("printed as a different tree: {}", &text[..text.len().min(200)])),
        Err(e) => Err(format!("printed as {} bytes that do not parse: {e}", text.len())),
    }
}

#[test]
fn review_an_index_over_256_bytes_is_printed_in_quotes() {
    // An unquoted index is read to 256 bytes (`MAX_INDEX_BYTES`); a quoted name to 1,024. So the printer quotes what is longer
    // than the first, and what it prints reads back.
    for len in [255, 256, 257, 300, 1000, 1024] {
        let src = format!("A['{}']", "x".repeat(len));
        assert_eq!(read_back(&src), Ok(()), "{len}");
    }
    let long = "x".repeat(257);
    assert_eq!(printed(&format!("A['{long}']")), format!("A['{long}']"));
    // 256 is the last that is bare, and a whole number that long is a name, quoted anyway.
    let edge = "x".repeat(256);
    assert_eq!(printed(&format!("A['{edge}']")), format!("A[{edge}]"));
    assert_eq!(printed("A['2']"), "A['2']");
    // Also behind a qualifier and in a call's qualifier path.
    assert_eq!(read_back(&format!("Pages['{long}']!Sheet.1!Width")), Ok(()));
}

#[test]
fn review_a_printed_formula_is_not_nested_deeper_than_its_text() {
    // 100 brackets round a `3 IN ^ 2` is nesting 100; a cosmetic bracket on the literal made 101, and `TooDeep`.
    let src = format!("{}3 IN ^ 2{}", "1*(1+".repeat(100), ")".repeat(100));
    assert_eq!(read_back(&src), Ok(()));
    let src = format!("{}3 in^x{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(read_back(&src), Ok(()));
    // The bracket a rank needs was a bracket of the text, so the nesting of the print is the nesting of the text, to the limit.
    for depth in [1, 50, 99, 100] {
        let src = format!("{}3 IN ^ 2{}", "1*(1+".repeat(depth), ")".repeat(depth));
        assert_eq!(read_back(&src), Ok(()), "{depth}");
    }
}

#[test]
fn review_a_large_number_is_not_printed_in_full() {
    // `1e308` is 5 bytes of text and 309 of digits: 3,500 of them are 21 KB of text and over 1 MiB of digits, which is `TooLong`.
    let src = format!("F({})", vec!["1e308"; 3500].join(","));
    assert!(src.len() < 22_000);
    assert_eq!(read_back(&src), Ok(()));
    let printed_len = print(&ok(&src)).len();
    assert!(printed_len < 4 * src.len(), "{printed_len}");
    // The same of a tiny one.
    let src = format!("F({})", vec!["5e-324"; 3500].join(","));
    assert_eq!(read_back(&src), Ok(()));
    assert!(print(&ok(&src)).len() < 4 * src.len());
    // Exactly, whatever the size.
    let below = |v: f64| f64::from_bits(v.to_bits() - 1);
    for v in [1e16, below(1e16), 1e-5, below(1e-5), 9.99e-6, 1e22, 5e-324, f64::MAX, 1.7976931348623157e300, 123456789.0, 0.1 + 0.2] {
        let tree = n(v);
        assert_eq!(parse(&print(&tree)).ast, Ok(tree), "{v:e}");
    }
}

#[test]
fn review_what_print_writes_can_be_longer_than_the_limit_on_text() {
    // The documented limit of the round trip: `print` is for checking a parse and for messages, and its text can be longer than
    // the text it was made from (blanks round an operator, a unit's blank, `, ` for `,`). A formula near `MAX_SOURCE_BYTES` can
    // print past it and then not parse; a message does not need it to. Below the limit with room to spare it always does.
    let src = format!("F(\"{}\"{})", "x".repeat(500_000), ",1in".repeat(100_000));
    assert!(src.len() < MAX_SOURCE_BYTES);
    let tree = ok(&src);
    let text = print(&tree);
    assert!(text.len() > MAX_SOURCE_BYTES, "{}", text.len());
    assert_eq!(parse(&text).ast.err().map(|e| e.kind), Some(diacraft_formula::ParseErrorKind::TooLong));
    // With room for the blanks it reads back.
    let src = format!("F(\"{}\"{})", "x".repeat(100_000), ",1in".repeat(100_000));
    assert_eq!(read_back(&src), Ok(()));
}
