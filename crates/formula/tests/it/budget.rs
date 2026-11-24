//! A budget of nodes across formulas: the limits of the crate are per formula, so a document of many cells needs its own total
//! (`Limits` of the reader, Task 5). `Expr::node_count` says what a tree costs by the count `MAX_NODES` limits, and
//! `parse_with_budget` refuses a formula that would pass a budget.

use diacraft_formula::{Expr, MAX_NODES, ParseErrorKind, parse, parse_with_budget};

use crate::{err, ok};

const GUID: &str = "{00000000-1111-2222-3333-444444444444}";

#[test]
fn node_count_is_the_count_the_limit_counts() {
    // Every expression, every path segment and every empty slot counts one; a call counts itself and its name.
    for (src, nodes) in [
        ("1", 1),
        ("-1", 2),
        ("1+2", 3),
        ("40%", 2),
        ("(1+2)*3", 5),
        ("Width", 2),
        ("Prop.Cost.Value", 4),
        ("Sheet.12!PinX", 4),
        ("Pages[Site-2]!Sheet.9!EndY", 5),
        ("LISTSHEETREF(2)!Width", 4),
        ("F()", 2),
        ("F(1,,3)", 5),
        ("F(,)", 4),
        ("IF(a>1,2,3)", 8),
        ("Sheet.5!SHEETREF()", 4),
        ("\"x\"&\"y\"", 3),
        ("3 IN^2", 1),
        ("TRUE", 1),
    ] {
        assert_eq!(ok(src).node_count(), nodes, "{src}");
    }
    assert_eq!(ok(&format!("USE({GUID})")).node_count(), 3);
}

#[test]
fn a_budget_of_exactly_the_count_parses_and_one_less_does_not() {
    for src in [
        "1",
        "1+2*3",
        "F(1,,3)",
        "IF(a>1,2,3)",
        "Sheet.12!PinX",
        "-(-1)%",
        "A.B.C.D",
        "LISTSHEETREF(2)!Width",
        "G(F(,,),\"s\")&Width",
        "TRUE",
        "F(TRUE,FALSE,x)",
        &format!("USE({GUID})"),
    ] {
        let nodes = ok(src).node_count();
        let at = parse_with_budget(src, nodes);
        assert_eq!(at.ast.as_ref().ok().map(Expr::node_count), Some(nodes), "{src}");
        assert_eq!(at.node_count(), nodes);
        let under = parse_with_budget(src, nodes - 1);
        assert_eq!(under.ast.as_ref().err().map(|e| e.kind), Some(ParseErrorKind::TooManyNodes), "{src}");
        // The source is kept, whatever the budget did, and a failed parse costs nothing.
        assert_eq!(&*under.source, src);
        assert_eq!(under.node_count(), 0);
    }
    assert_eq!(parse_with_budget("1", 0).ast.err().map(|e| e.kind), Some(ParseErrorKind::TooManyNodes));
    // No budget is a parse: the crate's own limit.
    assert_eq!(parse_with_budget("1+2", usize::MAX), parse("1+2"));
    assert_eq!(parse_with_budget("1+2", MAX_NODES), parse("1+2"));
}

#[test]
fn a_budget_cannot_raise_the_limit_of_the_crate() {
    let src = format!("F({})", vec!["A"; MAX_NODES].join(","));
    assert_eq!(parse_with_budget(&src, usize::MAX).ast.err().map(|e| e.kind), Some(ParseErrorKind::TooManyNodes));
    assert_eq!(err(&src).kind, ParseErrorKind::TooManyNodes);
}

#[test]
fn a_small_budget_stops_a_large_formula_early() {
    // The case of the review: `F(A,A,...)` of 130 KB is 65,000 names, 130,002 nodes, which cost about 9.4 million bytes as a tree.
    // With a budget the tree is not built past it: the parse stops at the node that passes the budget, and the offset says where.
    let src = format!("F({})", vec!["A"; 65_000].join(","));
    assert!(src.len() > 128_000);
    let small = parse_with_budget(&src, 1000);
    let e = small.ast.err();
    assert_eq!(e.map(|e| e.kind), Some(ParseErrorKind::TooManyNodes));
    // The name `F` is 1 and each `A` is 2 (its segment and itself): the 500th `A` makes node 1,001, and ends at byte 2 * 500 + 1. A
    // check made after the whole text was read would give an offset near the end, at 130,001.
    assert_eq!(e.map(|e| e.offset), Some(1001));
    // A parse that fits counts what it used: a name is its segment and itself, so a call with 65,000 names is 130,002.
    let all = parse(&src);
    assert_eq!(all.node_count(), 2 + 2 * 65_000);
    assert!(all.is_parsed());
}

#[test]
fn a_document_budget_is_spent_formula_by_formula() {
    // How the reader uses it: one total for the cells of a document, handed on as what is left. A formula past the rest keeps its
    // source and is an error class (`TooManyNodes`), and nothing after it can use what the document no longer has.
    let cells = ["1+2", "F(1,2,3)", "Width*Height", "IF(a>1,2,3)", "Sheet.1!PinX+Sheet.1!PinY", "1", "2"];
    let mut left = 20;
    let mut parsed = 0;
    let mut refused = 0;
    for src in cells {
        let f = parse_with_budget(src, left);
        match &f.ast {
            Ok(_) => {
                left -= f.node_count();
                parsed += 1;
            }
            Err(e) => {
                assert_eq!(e.kind, ParseErrorKind::TooManyNodes, "{src}");
                refused += 1;
            }
        }
        assert_eq!(&*f.source, src);
    }
    assert_eq!((parsed, refused), (5, 2), "{left} left");
    assert!(left <= 20);
}

#[test]
fn node_count_does_not_recurse_without_a_bound() {
    // A tree built by hand can be as deep as its builder likes; counting it must not overflow the stack.
    let n = crate::within(60, || {
        let mut e = Expr::Bool(true);
        for _ in 0..200_000 {
            e = Expr::Percent(Box::new(e));
        }
        let n = e.node_count();
        // Taking it apart from the top, as `Drop` would recurse.
        let mut cur = e;
        while let Expr::Percent(inner) = cur {
            cur = *inner;
        }
        n
    });
    assert_eq!(n, 200_001);
}
