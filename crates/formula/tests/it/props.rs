//! Properties: any text gives a tree or an error and never panics; what `print` writes parses back to the tree it was given; and depth
//! and size stay inside the limits whatever the text.

use std::sync::Arc;

use diacraft_formula::{
    BinaryOp, CURRENCIES, Call, CellRef, Expr, ImplicitMeasure, Index, MAX_DEPTH, MAX_NODES, Number, Qualifier, Segment, UnaryOp, parse, print,
};
use diacraft_units::{Measure, Unit};
use proptest::prelude::*;

/// The units a literal can be written with, one spelling each: not the percent (it is the postfix operator), the compounds, the
/// date and the ruler variants, which no text gives.
const LITERAL_UNITS: [Unit; 27] = [
    Unit::Inch,
    Unit::Foot,
    Unit::Yard,
    Unit::Mile,
    Unit::NauticalMile,
    Unit::Millimetre,
    Unit::Centimetre,
    Unit::Metre,
    Unit::Kilometre,
    Unit::DrawingDefault,
    Unit::PageDefault,
    Unit::Point,
    Unit::TypeDefault,
    Unit::Pica,
    Unit::Didot,
    Unit::Cicero,
    Unit::Degree,
    Unit::Radian,
    Unit::AngleDefault,
    Unit::ElapsedDay,
    Unit::ElapsedHour,
    Unit::ElapsedMinute,
    Unit::ElapsedSecond,
    Unit::ElapsedWeek,
    Unit::DurationDefault,
    Unit::Acre,
    Unit::Hectare,
];

/// A number a literal can hold: finite, not negative, and from the places that break reading and printing.
fn value() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => 0.0f64..1000.0,
        2 => (0u32..100_000).prop_map(f64::from),
        1 => Just(0.0),
        1 => Just(0.1),
        1 => Just(1e21),
        1 => Just(1e300),
        1 => Just(5e-324),
        1 => Just(f64::MAX),
        1 => 0.0f64..1e-3,
        1 => (0u32..1000, 1u32..1000).prop_map(|(a, b)| f64::from(a) / f64::from(b)),
    ]
}

fn unit() -> impl Strategy<Value = Unit> {
    prop::sample::select(LITERAL_UNITS.to_vec())
}

/// A unit that takes a power straight after it: the list of bases, which leaves out the acre and the hectare.
fn power_base() -> impl Strategy<Value = Unit> {
    prop::sample::select(LITERAL_UNITS.iter().copied().filter(|u| u.measure() != Measure::Area).collect::<Vec<_>>())
}

fn number() -> impl Strategy<Value = Number> {
    prop_oneof![
        3 => value().prop_map(Number::plain),
        3 => (value(), unit()).prop_map(|(v, u)| Number::with_unit(v, u)),
        2 => (value(), power_base(), -200i32..200).prop_map(|(value, unit, p)| Number::Measured { value, unit, power: Some(p) }),
        1 => (value(), unit(), prop::sample::select(vec![
            ImplicitMeasure::Angle,
            ImplicitMeasure::Drawing,
            ImplicitMeasure::Duration,
            ImplicitMeasure::Page,
            ImplicitMeasure::Type,
        ]))
            .prop_map(|(value, unit, measure)| Number::Implicit { value, unit, measure }),
        1 => (value(), value()).prop_map(|(feet, inches)| Number::FeetInches { feet, inches }),
        1 => (value(), value(), value()).prop_map(|(degrees, minutes, seconds)| Number::DegreesMinutesSeconds { degrees, minutes, seconds }),
        1 => (value(), value()).prop_map(|(ciceros, didots)| Number::CiceroDidot { ciceros, didots }),
        1 => (value(), value()).prop_map(|(picas, points)| Number::PicaPoints { picas, points }),
    ]
}

fn ident() -> impl Strategy<Value = String> {
    "[A-Za-z_][A-Za-z0-9_]{0,6}"
}

/// A name of any characters a quoted name can hold.
fn any_name() -> impl Strategy<Value = String> {
    "[ -~éü日]{1,8}"
}

fn name() -> impl Strategy<Value = String> {
    prop_oneof![3 => ident(), 1 => any_name(), 1 => Just("TRUE".to_string()), 1 => Just("false".to_string()), 1 => "[0-9]{1,3}"]
}

fn index() -> impl Strategy<Value = Index> {
    prop_oneof![any::<u32>().prop_map(Index::Number), (0u32..20).prop_map(Index::Number), any_name().prop_map(|s| Index::Name(s.into()))]
}

fn segment() -> impl Strategy<Value = Segment> {
    (name(), prop::option::weighted(0.3, index())).prop_map(|(n, index)| Segment { name: n.into(), index })
}

fn path() -> impl Strategy<Value = Vec<Segment>> {
    prop::collection::vec(segment(), 1..4)
}

/// Argument slots as a text can give them. One empty slot is not among them: `F()` is no slots at all.
fn args(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = Vec<Option<Expr>>> {
    prop::collection::vec(prop::option::weighted(0.8, inner), 0..4).prop_map(|mut slots| {
        if slots == [None] {
            slots.clear();
        }
        slots
    })
}

fn qualifier(inner: BoxedStrategy<Expr>) -> impl Strategy<Value = Qualifier> {
    prop_oneof![path().prop_map(Qualifier::Path), (ident(), args(inner)).prop_map(|(name, args)| Qualifier::Call { name: name.into(), args }),]
}

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        4 => number().prop_map(Expr::Number),
        1 => any::<bool>().prop_map(Expr::Bool),
        2 => "[ -~éü日]{0,8}".prop_map(|s| Expr::Str(Arc::from(s))),
        1 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Expr::Color { r, g, b }),
        // A currency holds a code of any of the 192: the parser reads no other, and a tree is only promised to print and read
        // back when a text could give it. This was `["USD", "EUR", "gbp", "XYZ"]`.
        1 => (value(), prop::sample::select(CURRENCIES.iter().map(|c| c.code).collect::<Vec<_>>()))
            .prop_map(|(value, code)| Expr::Currency { value, code: code.into() }),
        4 => path().prop_map(|path| Expr::Ref(Box::new(CellRef { qualifiers: vec![], path }))),
        // A GUID only stands as the one argument of `USE`, with its braces in its text.
        1 => (prop::sample::select(vec!["USE", "use", "Use"]), "\\{[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\\}")
            .prop_map(|(name, guid)| Expr::Call(Box::new(Call { qualifiers: vec![], name: name.into(), args: vec![Some(Expr::Guid(guid.into()))] }))),
    ]
}

fn unary_op() -> impl Strategy<Value = UnaryOp> {
    prop::sample::select(vec![UnaryOp::Plus, UnaryOp::Minus])
}

fn binary_op() -> impl Strategy<Value = BinaryOp> {
    prop::sample::select(vec![
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
    ])
}

/// A tree of anything a text can give.
fn expr() -> impl Strategy<Value = Expr> {
    leaf().prop_recursive(5, 48, 4, |inner| {
        let inner = inner.boxed();
        prop_oneof![
            2 => (unary_op(), inner.clone()).prop_map(|(op, e)| Expr::Unary(op, Box::new(e))),
            4 => (binary_op(), inner.clone(), inner.clone()).prop_map(|(op, a, b)| Expr::Binary(op, Box::new(a), Box::new(b))),
            1 => inner.clone().prop_map(|e| Expr::Percent(Box::new(e))),
            2 => (prop::collection::vec(qualifier(inner.clone()), 0..2), ident(), args(inner.clone()))
                .prop_map(|(qualifiers, name, args)| Expr::Call(Box::new(Call { qualifiers, name: name.into(), args }))),
            1 => (prop::collection::vec(qualifier(inner.clone()), 1..3), path())
                .prop_map(|(qualifiers, path)| Expr::Ref(Box::new(CellRef { qualifiers, path }))),
        ]
    })
}

/// Pieces of formula text: put together at random they give formulas that parse, ones that nearly do, and ones that do not.
fn piece() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "0",
        "1",
        "2",
        "2.5",
        ".5",
        "5.",
        "1e3",
        "1E-2",
        "1E",
        "1EM",
        "007",
        "in",
        "cm",
        "FT",
        "ft",
        "DEG",
        "IN^2",
        "in^-1",
        "^2",
        "%",
        "%%",
        "+",
        "-",
        "--",
        "*",
        "/",
        "^",
        "&",
        "<",
        "<=",
        ">",
        ">=",
        "=",
        "<>",
        "_GT_",
        "GT",
        "_eq_",
        "NE",
        "(",
        ")",
        "((",
        "))",
        ",",
        ",,",
        ".",
        "!",
        "[",
        "]",
        "[2]",
        "[x y]",
        "'",
        "''",
        "'Left-Door'",
        "'O''B'",
        "\"",
        "\"\"",
        "\"abc\"",
        "\"a\"\"b\"",
        "#FF8000",
        "#ff",
        "#REF!",
        "#N/A",
        "#",
        "{ab-cd}",
        "{",
        "}",
        "°",
        "15'",
        "30\"",
        "5 1/2",
        "3[cm,D]",
        "[cm,d]",
        "Width",
        "Height",
        "A",
        "x",
        "Scratch.A1",
        "Prop.Cost.Value",
        "Char.Size[2]",
        "Sheet.12!",
        "Pages[Site-2]!",
        "_X",
        "TRUE",
        "false",
        "SUM(",
        "F(",
        "IF(",
        "_ADD(",
        "LISTSHEETREF(2)!",
        "GetRef(",
        "USD",
        "ACRE",
        "SQ. IN.",
        "2C4",
        "3P6",
        "8FT 4IN",
        " ",
        "  ",
        "\t",
        "\n",
        "=",
        ";",
        "?",
        "é",
        "\u{0}",
    ])
    .prop_map(str::to_string)
}

fn soup() -> impl Strategy<Value = String> {
    (prop::collection::vec((piece(), prop::sample::select(vec!["", "", " "])), 0..9)).prop_map(|parts| {
        let mut s = String::new();
        for (piece, sep) in parts {
            s.push_str(&piece);
            s.push_str(sep);
        }
        s
    })
}

/// The text of a tree, edited at random: cut short, with a character taken out, with a piece put in or a character changed. Most
/// of it still parses, and the rest fails near where it was cut, which is where a parser breaks.
fn mutated() -> impl Strategy<Value = String> {
    (expr(), prop::collection::vec((any::<prop::sample::Index>(), 0u8..4, piece()), 0..3)).prop_map(|(e, edits)| {
        let mut chars: Vec<char> = print(&e).chars().collect();
        for (at, kind, piece) in edits {
            let i = at.index(chars.len() + 1);
            match kind {
                0 => chars.truncate(i),
                1 => {
                    if i < chars.len() {
                        chars.remove(i);
                    }
                }
                2 => {
                    for (k, c) in piece.chars().enumerate() {
                        chars.insert((i + k).min(chars.len()), c);
                    }
                }
                _ => {
                    if let Some(slot) = chars.get_mut(i) {
                        *slot = piece.chars().next().unwrap_or(' ');
                    }
                }
            }
        }
        chars.into_iter().collect()
    })
}

/// The depth of a tree as the crate counts it, and its node count (the crate counts a path segment as one, and an argument slot).
fn measure(e: &Expr) -> (usize, usize) {
    fn segs(path: &[Segment]) -> usize {
        path.len()
    }
    fn arg_list(a: &[Option<Expr>]) -> (usize, usize) {
        a.iter().fold((0, 0), |(d, n), x| match x {
            Some(x) => {
                let (xd, xn) = measure(x);
                (d.max(xd), n + xn)
            }
            None => (d, n + 1),
        })
    }
    fn quals(q: &[Qualifier]) -> (usize, usize) {
        q.iter().fold((0, 0), |(d, n), q| match q {
            Qualifier::Path(p) => (d, n + 1 + segs(p)),
            Qualifier::Call { args, .. } => {
                let (ad, an) = arg_list(args);
                (d.max(ad), n + 1 + an)
            }
        })
    }
    match e {
        Expr::Unary(_, x) | Expr::Percent(x) => {
            let (d, n) = measure(x);
            (d + 1, n + 1)
        }
        Expr::Binary(_, a, b) => {
            let ((ad, an), (bd, bn)) = (measure(a), measure(b));
            (ad.max(bd) + 1, an + bn + 1)
        }
        Expr::Call(c) => {
            let ((ad, an), (qd, qn)) = (arg_list(&c.args), quals(&c.qualifiers));
            (ad.max(qd) + 1, an + qn + 1)
        }
        Expr::Ref(r) => {
            let (qd, qn) = quals(&r.qualifiers);
            (qd + 1, qn + segs(&r.path))
        }
        _ => (1, 1),
    }
}

/// How many cases each property tries: 400, or what `DIACRAFT_PROPTEST_CASES` says, for a longer hunt.
fn cases() -> u32 {
    std::env::var("DIACRAFT_PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(400)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// What `print` writes parses back to the tree it was given, for every tree a text can give.
    #[test]
    fn print_then_parse_is_the_tree(e in expr()) {
        let text = print(&e);
        let again = parse(&text);
        prop_assert_eq!(&*again.source, &*text);
        prop_assert_eq!(again.ast, Ok(e), "printed as {:?}", text);
    }

    /// Printing is a fixed point: the text of a reparse is the text.
    #[test]
    fn printing_twice_gives_one_text(e in expr()) {
        let once = print(&e);
        let twice = print(&parse(&once).ast.map_err(|e| TestCaseError::fail(format!("{once:?}: {e}")))?);
        prop_assert_eq!(once, twice);
    }

    /// Formulas made of pieces: whatever they give, a tree prints and parses back to itself, an error says where, and the
    /// source is kept.
    #[test]
    fn pieces_of_formulas_never_panic_and_round_trip(text in soup()) {
        let f = parse(&text);
        prop_assert_eq!(&*f.source, &*text);
        match f.ast {
            Ok(tree) => {
                let printed = print(&tree);
                prop_assert_eq!(parse(&printed).ast, Ok(tree), "{:?} printed as {:?}", text, printed);
            }
            Err(e) => prop_assert!(e.offset <= text.len(), "{e} in {text:?}"),
        }
    }

    /// The text of a tree with edits in it: what still parses prints and parses back to itself, and what does not says where.
    #[test]
    fn edited_formulas_round_trip_or_fail_with_an_offset(text in mutated()) {
        let f = parse(&text);
        prop_assert_eq!(&*f.source, &*text);
        match f.ast {
            Ok(tree) => {
                let printed = print(&tree);
                prop_assert_eq!(parse(&printed).ast, Ok(tree), "{:?} printed as {:?}", text, printed);
            }
            Err(e) => prop_assert!(e.offset <= text.len(), "{e} in {text:?}"),
        }
    }

    /// Any text at all, of any characters.
    #[test]
    fn any_text_gives_a_tree_or_an_error(text in "\\PC{0,200}") {
        let f = parse(&text);
        prop_assert_eq!(&*f.source, &*text);
        if let Err(e) = f.ast {
            prop_assert!(e.offset <= text.len());
        }
    }

    /// Text of the characters of formulas, long.
    #[test]
    fn formula_characters_never_panic(text in "[0-9A-Za-z_ .,()\\[\\]!'\"#{}%^&*/+<>=°-]{0,400}") {
        let f = parse(&text);
        if let Ok(tree) = &f.ast {
            let printed = print(tree);
            prop_assert_eq!(parse(&printed).ast, Ok(tree.clone()), "{:?} printed as {:?}", text, printed);
        }
    }

    /// Depth and size stay inside the limits whatever nests.
    #[test]
    fn depth_and_size_are_limited(pieces in prop::collection::vec(prop::sample::select(vec!["(", "F(", "-", "%", "1", ")", "+", ",", "!", "A", "A.", " ", "^", "[", "]", "1 IN^2"]), 0..3000)) {
        let text: String = pieces.concat();
        if let Ok(tree) = parse(&text).ast {
            let (depth, nodes) = measure(&tree);
            prop_assert!(depth <= MAX_DEPTH, "depth {depth}");
            prop_assert!(nodes <= MAX_NODES, "{nodes} nodes");
        }
    }

    /// Text cut at any character, with multi-byte ones among them, parses or fails and never panics on a boundary.
    #[test]
    fn a_cut_in_a_multibyte_character_is_not_a_panic(prefix in "[0-9é日°]{0,40}", cut in 0usize..80) {
        let text = format!("{prefix}日é°");
        let end = text.char_indices().map(|(i, _)| i).chain(Some(text.len())).nth(cut % (text.chars().count() + 1)).unwrap_or(0);
        let f = parse(&text[..end]);
        prop_assert_eq!(&*f.source, &text[..end]);
    }
}

/// A length at an edge of what the parser reads: of a bare index (256), of a name (1,024), and on either side.
fn edge_length() -> impl Strategy<Value = usize> {
    prop::sample::select(vec![1usize, 2, 255, 256, 257, 258, 1000, 1023, 1024])
}

/// A name of `len` bytes: letters, or with a character that needs quotes in it.
fn name_of(len: usize, quoted: bool) -> String {
    if quoted && len > 1 { format!("{}-", "n".repeat(len - 1)) } else { "n".repeat(len) }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// Names and indexes of the lengths at the edges of what is read: what is printed is read again (an index over 256 bytes is
    /// quoted, as a quoted name may be 1,024).
    #[test]
    fn names_and_indexes_at_their_limits_round_trip(
        first in edge_length(),
        first_quoted in any::<bool>(),
        last in edge_length(),
        last_quoted in any::<bool>(),
        index in edge_length(),
        index_quoted in any::<bool>(),
        numbered in any::<bool>(),
    ) {
        let index = if numbered { Index::Number(u32::MAX) } else { Index::Name(name_of(index, index_quoted).into()) };
        let path = vec![
            Segment { name: name_of(first, first_quoted).into(), index: None },
            Segment { name: name_of(last, last_quoted).into(), index: Some(index) },
        ];
        let tree = Expr::Ref(Box::new(CellRef { qualifiers: vec![Qualifier::Path(vec![Segment::new(&name_of(first, true))])], path }));
        let text = print(&tree);
        prop_assert_eq!(parse(&text).ast, Ok(tree), "printed as {} bytes", text.len());
    }

    /// Chains and nests at the edges of the limits round trip: a left chain of any length to `MAX_DEPTH` terms, brackets, calls and
    /// signs to `MAX_NESTING`, each with an operator or a bracket kind picked at random, and one past the edge is refused.
    #[test]
    fn chains_and_nests_at_their_limits_round_trip(
        terms in prop::sample::select(vec![1usize, 2, 3, 100, 255, 256, 257, 510, 511, 512, 513, 514]),
        op in binary_op(),
        levels in prop::sample::select(vec![1usize, 2, 50, 98, 99, 100, 101, 102]),
        kind in 0u8..3,
    ) {
        let symbol = op.symbol();
        let chain = vec!["1"; terms].join(&format!(" {symbol} "));
        let nest = match kind {
            0 => format!("{}1{}", "(1+".repeat(levels), ")".repeat(levels)),
            1 => format!("{}1{}", "F(".repeat(levels), ")".repeat(levels)),
            _ => format!("{}1", "-".repeat(levels)),
        };
        for (text, within) in [(chain, terms <= MAX_DEPTH), (nest, levels <= diacraft_formula::MAX_NESTING)] {
            match parse(&text).ast {
                Ok(tree) => {
                    prop_assert!(within, "{} bytes read past a limit", text.len());
                    let printed = print(&tree);
                    prop_assert_eq!(parse(&printed).ast, Ok(tree));
                }
                Err(e) => {
                    prop_assert!(!within, "{} bytes refused inside the limits: {e}", text.len());
                    prop_assert_eq!(e.kind, diacraft_formula::ParseErrorKind::TooDeep);
                }
            }
        }
    }
}
