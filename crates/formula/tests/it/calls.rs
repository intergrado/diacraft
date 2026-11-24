//! Calls, and every function name the index and the census list.

use diacraft_formula::{Expr, ParseErrorKind, parse, print};

use crate::names::{CENSUS_FUNCTIONS, INDEX_FUNCTIONS, SPEC_EXAMPLES, census_functions, index_functions};
use crate::{err, ok, shape};

#[test]
fn the_lists_are_the_size_the_specs_give() {
    assert_eq!(index_functions().len(), 256, "256 functions");
    assert_eq!(census_functions().len(), 186, "186 distinct function names");
    let mut census = census_functions();
    census.sort_unstable();
    census.dedup();
    assert_eq!(census.len(), 186);
    assert!(INDEX_FUNCTIONS.contains("_SHAPEMIN") && CENSUS_FUNCTIONS.contains("GetRef"));
}

#[test]
fn every_function_of_the_index_is_a_call_with_or_without_arguments() {
    for name in index_functions() {
        for (src, args) in [(format!("{name}()"), 0), (format!("{name}(1,2)"), 2), (format!("{name}( 1 , Width )"), 2), (format!("{name}(,)"), 2)] {
            let Expr::Call(c) = ok(&src) else { panic!("{src}") };
            assert_eq!((&*c.name, c.args.len()), (name, args), "{src}");
            assert!(c.qualifiers.is_empty());
        }
        // And glued to a sheet qualifier, and with nothing after it: a reference, which is no call.
        let Expr::Call(c) = ok(&format!("Sheet.1!{name}()")) else { panic!("{name}") };
        assert_eq!(&*c.name, name);
        assert_eq!(c.qualifiers.len(), 1);
        assert!(matches!(ok(name), Expr::Ref(_) | Expr::Bool(_)), "{name}");
    }
}

#[test]
fn every_function_name_in_files_is_a_call_known_or_not() {
    // The census: 186 names, some with underscores (`_MOD`, `_ELLIPSE_THETA`) and some in mixed case. A name the parser has
    // never heard of is still a call, so no list stands between a file and its parse.
    for name in census_functions() {
        let src = format!("{name}(Width, 2 in, \"x\", #FF8000)");
        let Expr::Call(c) = ok(&src) else { panic!("{src}") };
        assert_eq!((&*c.name, c.args.len()), (name, 4), "{src}");
        assert!(c.is(&name.to_ascii_lowercase()) && c.is(&name.to_ascii_uppercase()));
    }
    // Names made up on the spot, in the shapes the census shows: capitals, mixed case, underscores, digits.
    for name in ["XYZZY", "Frobnicate9", "_UCON_ZZ", "a", "A_B_C_1", "_X", "__", "_1", "Z9", "é", "MixedCase"] {
        assert_eq!(shape(&format!("{name}(1)")), format!("(call {name} 1)"));
    }
}

#[test]
fn the_names_the_census_gives_in_mixed_case_keep_their_case() {
    for name in ["GetRef", "GetVal", "SheetRef"] {
        let Expr::Call(c) = ok(&format!("{name}(Scratch.A1)")) else { panic!() };
        assert_eq!(&*c.name, name);
    }
}

#[test]
fn the_arguments_of_a_call_are_full_expressions() {
    assert_eq!(shape("IF(Height > 1.25 in,5,7)"), "(call IF (> @Height 1.25IN) 5 7)");
    assert_eq!(shape("IF(4>2,NA(),7)"), "(call IF (> 4 2) (call NA) 7)");
    assert_eq!(shape("MAX(1+2*3, -4, (5))"), "(call MAX (+ 1 (* 2 3)) (neg 4) 5)");
    assert_eq!(shape("F(G(H(1)), 2)"), "(call F (call G (call H 1)) 2)");
    assert_eq!(shape("F(\"a,b\", \"(\", \")\")"), "(call F \"a,b\" \"(\" \")\")");
    assert_eq!(shape("IF(#FF8000,5,7)"), "(call IF #FF8000 5 7)");
    assert_eq!(shape("BOUND(15, 0, TRUE, 1, 10, FALSE, 20, 30)"), "(call BOUND 15 0 TRUE 1 10 FALSE 20 30)");
    assert_eq!(shape("IF(Scratch.A1>5,SETF(GetRef(Scratch.A1),5),0)"), "(call IF (> @Scratch.A1 5) (call SETF (call GetRef @Scratch.A1) 5) 0)");
    // A qualified reference and a call as arguments.
    assert_eq!(shape("F(Sheet.1!Width, G(2)!Height)"), "(call F @Sheet.1!Width @G(2)!Height)");
}

#[test]
fn white_space_may_sit_between_a_name_and_its_brackets() {
    // By example only. No file has one (the census says none does), and nothing else a name and a bracket could mean is
    // a formula, so it is a call.
    assert_eq!(shape("PI ()"), "(call PI)");
    assert_eq!(shape("F (1)"), "(call F 1)");
    assert_eq!(shape("F\n(1)"), "(call F 1)");
}

#[test]
fn the_spelling_of_true_and_false_before_a_bracket_is_a_name() {
    // `TRUE` alone is a word; `TRUE(1)` is a name before brackets, so a call (R8 takes any name before an argument list).
    assert_eq!(shape("TRUE(1)"), "(call TRUE 1)");
}

#[test]
fn a_call_with_a_bad_list_is_an_error() {
    for (src, kind) in [
        ("F(1,2", ParseErrorKind::ExpectedArgumentSeparator),
        ("F(1 2)", ParseErrorKind::ExpectedArgumentSeparator),
        ("F(1,2))", ParseErrorKind::ExpectedOperator),
        ("F((1)", ParseErrorKind::ExpectedArgumentSeparator),
        ("F(1,*)", ParseErrorKind::ExpectedOperand),
        ("F(+)", ParseErrorKind::ExpectedOperand),
        ("F(", ParseErrorKind::UnexpectedEnd),
        ("F(1,", ParseErrorKind::UnexpectedEnd),
        ("1(2)", ParseErrorKind::ExpectedOperator),
        ("F(1)(2)", ParseErrorKind::ExpectedOperator),
        ("(F)(2)", ParseErrorKind::ExpectedOperator),
        ("\"F\"(2)", ParseErrorKind::ExpectedOperator),
    ] {
        assert_eq!(err(src).kind, kind, "{src}");
    }
}

#[test]
fn the_examples_of_the_specs_parse_and_print_to_the_same_tree() {
    // About 450 formulas from the vectors.
    // Those that read oddly (`ADD(13,29)`, `_ADD(13,)`, `4+"goat"`) are parses: the value is `calc`'s.
    assert!(SPEC_EXAMPLES.len() > 400, "{}", SPEC_EXAMPLES.len());
    for src in SPEC_EXAMPLES {
        let tree = ok(src);
        let text = print(&tree);
        let again = match parse(&text).ast {
            Ok(t) => t,
            Err(e) => panic!("{src:?} prints as {text:?}, which does not parse: {e}"),
        };
        assert_eq!(tree, again, "{src:?} prints as {text:?}");
        // The source is the text given, untouched.
        assert_eq!(&*parse(src).source, *src);
    }
}
