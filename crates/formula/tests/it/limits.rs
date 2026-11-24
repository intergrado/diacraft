//! Hostile text: depth, size and work are bounded, and nothing panics, overflows the stack or hangs (AGENTS.md, Never crash).
//!
//! The limits are `MAX_SOURCE_BYTES`, `MAX_NESTING`, `MAX_DEPTH` and `MAX_NODES`; says no page gives
//! The reference app's own, so these are ours.

use diacraft_formula::{Expr, MAX_DEPTH, MAX_NESTING, MAX_NODES, MAX_SOURCE_BYTES, ParseErrorKind, parse};

use crate::{err, ok, within, within_stack};

/// The depth of a tree as the crate counts it: a leaf is 1.
fn depth(e: &Expr) -> usize {
    1 + match e {
        Expr::Unary(_, x) | Expr::Percent(x) => depth(x),
        Expr::Binary(_, a, b) => depth(a).max(depth(b)),
        Expr::Call(c) => c.args.iter().flatten().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[test]
fn brackets_nest_to_the_limit_and_no_further() {
    let nested = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
    assert_eq!(ok(&nested(MAX_NESTING)), ok("1"));
    assert_eq!(err(&nested(MAX_NESTING + 1)).kind, ParseErrorKind::TooDeep);
    // Calls and signs count too.
    let calls = |n: usize| format!("{}1{}", "F(".repeat(n), ")".repeat(n));
    assert!(parse(&calls(MAX_NESTING)).ast.is_ok());
    assert_eq!(err(&calls(MAX_NESTING + 1)).kind, ParseErrorKind::TooDeep);
    let signs = |n: usize| format!("{}1", "-".repeat(n));
    assert!(parse(&signs(MAX_NESTING)).ast.is_ok());
    assert_eq!(err(&signs(MAX_NESTING + 1)).kind, ParseErrorKind::TooDeep);
    // Mixed: each opening counts once whatever it is.
    assert_eq!(err(&format!("{}1", "(-F(".repeat(40))).kind, ParseErrorKind::TooDeep);
}

#[test]
fn a_hostile_nest_is_an_error_not_a_stack_overflow() {
    let (a, b, c, d) = within(60, || {
        let r = |s: String| parse(&s).ast.err().map(|e| e.kind);
        (
            r("(".repeat(500_000)),
            r("-".repeat(500_000)),
            r("F(".repeat(250_000)),
            // The same, closed.
            r(format!("{}1{}", "(".repeat(200_000), ")".repeat(200_000))),
        )
    });
    for kind in [a, b, c, d] {
        assert_eq!(kind, Some(ParseErrorKind::TooDeep));
    }
}

#[test]
fn review_the_deepest_recursion_of_the_parser_fits_a_small_stack() {
    // The most frames the parser has for one level of nesting: the whole run of binary ranks, down to a sign, an atom and a call or
    // a bracket again, `1=1<1&1+1*1^F(` a hundred times. It is refused, but not early: the depth of the tree is known coming back
    // (`node` checks it on return, and one such level is 7 deep, so 512 is passed at about 73 levels from the innermost), so the
    // descent goes the whole way down to the nesting limit of 100 first, with every frame still on the stack (with a sign in the
    // opener, `-(`, a level counts two and the descent stops at 50). So the refusal and what is read up to it must not need more
    // than a stack of 512 KiB (what a worker or a pooled thread may have: a quarter of a test thread's default).
    for opener in ["F(", "(", "-(", "-F(", "F(1,"] {
        let unit = format!("1=1<1&1+1*1^{opener}");
        let (few, many, deepest) = within_stack(60, 512 * 1024, move || {
            let levels = |n: usize| format!("{}1{}", unit.repeat(n), ")".repeat(n));
            // The most levels read: the refusal comes by nesting (going down) or by depth (coming back up), whichever is first.
            let deepest = (1..=MAX_NESTING).take_while(|n| parse(&levels(*n)).ast.is_ok()).last().unwrap_or(0);
            (parse(&levels(30)).ast.is_ok(), parse(&levels(MAX_NESTING)).ast.err().map(|e| e.kind), deepest)
        });
        assert!(few, "{opener}");
        assert_eq!(many, Some(ParseErrorKind::TooDeep), "{opener}");
        assert!(deepest >= 30, "{opener}: {deepest}");
    }
}

#[test]
fn a_percent_run_is_bounded_too() {
    // Every `%` adds a level to the tree: a million of them cannot make a tree a million deep.
    let e = err(&format!("1{}", "%".repeat(1_000_000)));
    assert_eq!(e.kind, ParseErrorKind::TooDeep);
    assert!(parse(&format!("1{}", "%".repeat(MAX_DEPTH - 1))).ast.is_ok());
    assert_eq!(err(&format!("1{}", "%".repeat(MAX_DEPTH))).kind, ParseErrorKind::TooDeep);
}

#[test]
fn a_long_chain_is_a_deep_tree_and_is_limited_as_one() {
    // `a+a+a+...` groups from the left, so n terms make a tree n deep: the longest chain is MAX_DEPTH terms.
    let chain = |terms: usize, op: &str| vec!["1"; terms].join(op);
    for op in ["+", "-", "*", "/", "^", "&", "<", "=", " _GT_ "] {
        let tree = ok(&chain(MAX_DEPTH, op));
        assert_eq!(depth(&tree), MAX_DEPTH, "{op}");
        assert_eq!(err(&chain(MAX_DEPTH + 1, op)).kind, ParseErrorKind::TooDeep, "{op}");
    }
    // A million terms are 2 MB of text: over the length limit, and refused before they are read. 100,000 are under it and too deep.
    assert_eq!(err(&chain(1_000_000, "+")).kind, ParseErrorKind::TooLong);
    assert_eq!(err(&chain(100_000, "+")).kind, ParseErrorKind::TooDeep);
}

#[test]
fn the_tree_may_be_deep_where_the_nesting_is_not() {
    // A call's arguments each start a chain of their own: 100 of 400 terms cost 400, not 40,000 levels.
    let arg = vec!["1"; 400].join("+");
    let src = format!("F({})", vec![arg; 100].join(","));
    let tree = ok(&src);
    assert_eq!(depth(&tree), 401);
}

#[test]
fn text_over_the_length_limit_is_refused_unread() {
    let long = " ".repeat(MAX_SOURCE_BYTES + 1);
    let e = within(30, move || parse(&long));
    assert_eq!(e.ast.as_ref().err().map(|e| e.kind), Some(ParseErrorKind::TooLong));
    assert_eq!(e.ast.as_ref().err().map(|e| e.offset), Some(MAX_SOURCE_BYTES));
    // And the source is still kept whole.
    assert_eq!(e.source.len(), MAX_SOURCE_BYTES + 1);
    // At the limit it is read: all white space is an empty formula, not a long one.
    assert_eq!(err(&" ".repeat(MAX_SOURCE_BYTES)).kind, ParseErrorKind::Empty);
    // A multi-byte character that straddles the limit does not panic: one byte over is refused, and at the limit the text is read
    // (a megabyte of digits is one number far past the cap on a number's text).
    assert_eq!(err(&format!("{}é", "1".repeat(MAX_SOURCE_BYTES - 1))).kind, ParseErrorKind::TooLong);
    assert_eq!(err(&format!("{}é", "1".repeat(MAX_SOURCE_BYTES - 2))).kind, ParseErrorKind::NumberOutOfRange);
}

#[test]
fn a_tree_has_at_most_max_nodes_nodes() {
    // Every expression, every path segment and every empty slot counts one: a call is its name and itself, and then its arguments.
    let args = |n: usize| format!("F({})", vec!["1"; n].join(","));
    assert!(parse(&args(MAX_NODES - 2)).ast.is_ok());
    assert_eq!(err(&args(MAX_NODES - 1)).kind, ParseErrorKind::TooManyNodes);
    // An empty slot counts: a hundred thousand commas is as much a tree as the same number of ones (n commas are n + 1 slots).
    let slots = |n: usize| format!("F({})", ",".repeat(n));
    assert!(parse(&slots(MAX_NODES - 3)).ast.is_ok());
    assert_eq!(err(&slots(MAX_NODES - 2)).kind, ParseErrorKind::TooManyNodes);
    assert_eq!(err(&slots(MAX_NODES + 10)).kind, ParseErrorKind::TooManyNodes);
    // And so do path segments.
    let path = |n: usize| vec!["A"; n].join(".");
    assert!(parse(&path(1000)).ast.is_ok());
    assert_eq!(err(&path(MAX_NODES + 10)).kind, ParseErrorKind::TooManyNodes);
}

#[test]
fn a_formula_of_the_size_files_hold_parses() {
    // 20,000 numbers as the points of a polyline: far past what a real cell holds, well under the limits.
    let src = format!("POLYLINE(0, 0, {})", (0..20_000).map(|i| format!("{i}.5")).collect::<Vec<_>>().join(", "));
    let Expr::Call(c) = ok(&src) else { panic!() };
    assert_eq!(c.args.len(), 20_002);
    // And a chain of 300 terms: hundreds of cells added up.
    let src = (0..300).map(|i| format!("Scratch.A{i}")).collect::<Vec<_>>().join("+");
    assert_eq!(depth(&ok(&src)), 300);
}

#[test]
fn work_is_linear_in_the_text() {
    // Shapes that would cost more than their length if some step backed up over text it had read, each of a megabyte or so. The 60 s bound
    // below is a hang guard and no measure of cost: a megabyte read quadratically takes hours, the linear reading a few hundredths of a
    // second, so the minute is generous for a machine at load 50 and still tells one from the other.
    let cases: Vec<(&str, String)> = vec![
        ("a number and then spaces without end", format!("1{}$", " ".repeat(MAX_SOURCE_BYTES - 8))),
        ("numbers apart by spaces", "1 ".repeat(MAX_SOURCE_BYTES / 2 - 4)),
        ("a mixed number that never is", "1 2/".repeat(MAX_SOURCE_BYTES / 4 - 4)),
        ("a quote with no end", format!("'{}", "a".repeat(MAX_SOURCE_BYTES - 8))),
        ("an index with no end", format!("A[{}", "x".repeat(MAX_SOURCE_BYTES - 8))),
        ("a string with no end", format!("\"{}", "a".repeat(MAX_SOURCE_BYTES - 8))),
        ("a degree with no end", format!("1°{}", "1".repeat(MAX_SOURCE_BYTES - 8))),
        ("a GUID with no end", format!("{{{}", "a".repeat(MAX_SOURCE_BYTES - 8))),
        ("an unclosed implicit unit", "1[".repeat(MAX_SOURCE_BYTES / 2 - 4)),
        ("a unit lookahead that never ends", format!("1 FT {}", "9".repeat(MAX_SOURCE_BYTES - 16))),
        ("a power with digits without end", format!("1 IN^{}", "9".repeat(MAX_SOURCE_BYTES - 16))),
        ("letters after a number", format!("1 {}", "A".repeat(MAX_SOURCE_BYTES - 16))),
        ("comma after comma", format!("F({}", ",".repeat(MAX_SOURCE_BYTES - 8))),
        ("a name with a million letters", "A".repeat(MAX_SOURCE_BYTES - 8)),
        ("words that are operators", "a _GT_ ".repeat(MAX_SOURCE_BYTES / 8 - 4)),
        ("bangs", "A!".repeat(MAX_SOURCE_BYTES / 2 - 4)),
        ("dots", "A.".repeat(MAX_SOURCE_BYTES / 2 - 4)),
    ];
    for (what, text) in cases {
        let started = std::time::Instant::now();
        let kind = within(120, move || parse(&text).ast.err().map(|e| e.kind));
        // Every one is refused: none is a formula.
        assert!(kind.is_some(), "{what}");
        assert!(started.elapsed().as_secs() < 60, "{what} took {:?}", started.elapsed());
    }
}

#[test]
fn review_a_valid_megabyte_of_blanks_is_read_in_one_pass() {
    // The shapes above are refused, most of them early, so none could show a step that backs up over blanks it has read. These are
    // formulas that parse, with the blanks where a lookahead looks (after a number, a unit, a name, before `%`, `^`, `!`, a bracket
    // and a comma), a megabyte of text each. The 60 s bound is a hang guard and no measure of cost (a quadratic reading takes hours).
    let b = |n: usize| " ".repeat(n);
    let half = MAX_SOURCE_BYTES / 2 - 16;
    let tenth = MAX_SOURCE_BYTES / 10;
    let chain = (0..500).map(|_| "1").collect::<Vec<_>>().join(&format!("{}+{}", b(1000), b(1000)));
    let cases: Vec<(&str, String)> = vec![
        ("blanks after a number, then an operator and more", format!("1{}+{}1", b(half), b(half))),
        ("blanks in an argument list", format!("F(1{},{}2)", b(half), b(half))),
        ("a unit with blanks round it", format!("1{}IN{}+{}2 FT", b(tenth), b(tenth), b(tenth))),
        ("blanks before a percent", format!("1{}%{}%", b(half), b(half))),
        ("blanks before a bracket of a call", format!("F{}(1)", b(MAX_SOURCE_BYTES - 16))),
        ("blanks round a qualifier mark", format!("A{}!{}B", b(half), b(half))),
        ("blanks before what might be a mixed number", format!("1{}1/2", b(MAX_SOURCE_BYTES - 16))),
        ("blanks before a word operator", format!("1{}_GT_{}2", b(half), b(half))),
        ("blanks before a degree sign and after it", format!("1{}°{}", b(half), b(half))),
        ("blanks inside the bracket of an implicit unit", format!("1[{}in{},{}D{}]", b(tenth), b(tenth), b(tenth), b(tenth))),
        ("blanks before a power that is not a unit's", format!("1 IN{}^{}2", b(half), b(half))),
        ("500 terms apart by 2,000 blanks", chain),
    ];
    for (what, text) in cases {
        assert!(text.len() <= MAX_SOURCE_BYTES, "{what}: {}", text.len());
        let started = std::time::Instant::now();
        let parsed = within(120, move || parse(&text).ast.is_ok());
        assert!(parsed, "{what}");
        assert!(started.elapsed().as_secs() < 60, "{what} took {:?}", started.elapsed());
    }
}

#[test]
fn odd_characters_are_errors_not_panics() {
    for src in [
        "\u{0}",
        "\u{1}",
        "\u{7f}",
        "\u{feff}1",
        "1\u{feff}",
        "\u{200b}1",
        "\u{2028}",
        "\u{10ffff}",
        "\u{fffd}",
        "1 \u{301}",
        "a\u{301}",
        "é(1)",
        "\u{1f642}",
        "°",
        "1°°",
        "1°\"",
        "1°'",
        "'",
        "\"",
        "#",
        "{",
        "[",
        "]",
        "}",
        ")",
        "!",
        ".",
        ",",
        "%",
        "^",
        "&",
        "<",
        ">",
        "=",
        "<>",
        "<=",
        ">=",
        "1 <",
        "1 <>",
        "1 >=",
        "$",
        "@",
        "~",
        "`",
        "\\",
        ";",
        ":",
        "?",
        "|",
    ] {
        // Whatever it is, it is a result, and the source is kept.
        let f = parse(src);
        assert_eq!(&*f.source, src);
    }
}
