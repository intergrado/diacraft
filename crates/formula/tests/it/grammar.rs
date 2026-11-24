//! The grammar and the ranks: and its vectors, and
//! its vectors.

use diacraft_formula::{BinaryOp, Expr, PERCENT_FUNCTION, ParseErrorKind, UnaryOp, parse};

use crate::{bin, err, n, neg, ok, shape};

#[test]
fn a_typed_equals_sign_is_not_part_of_the_formula() {
    // Typed input may begin with one `=`; the file grammar has none. Same parse, and the source keeps what was given.
    assert_eq!(ok("=1+2"), ok("1+2"));
    assert_eq!(ok("  =  1+2"), ok("1+2"));
    assert_eq!(&*parse("=1+2").source, "=1+2");
    assert_eq!(shape("1+2"), "(+ 1 2)");
    // Only one: a second `=` is the operator, and has no left operand.
    assert_eq!(err("==1").kind, ParseErrorKind::ExpectedOperand);
    // `=` after an operand is the equality operator, with or without a first one.
    assert_eq!(shape("=1=2"), "(= 1 2)");
    assert_eq!(shape("1=2"), "(= 1 2)");
}

#[test]
fn the_source_is_kept_whether_or_not_it_parses() {
    for src in ["  1 + 2 ", "=\t5\n", "1 +", "((", "\"unterminated", "", "   ", "é", "#", "\u{0}", "Inh", "No Formula"] {
        assert_eq!(&*parse(src).source, src, "{src:?}");
    }
    assert!(!parse("1 +").is_parsed() && parse("1").is_parsed());
    // `Inh` is a reference like any other: the reader tells the file marker apart first.
    assert_eq!(shape("Inh"), "@Inh");
    assert_eq!(err("No Formula").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn nothing_is_an_empty_formula() {
    for src in ["", " ", "\t\n", "=", " = ", "=  "] {
        assert_eq!(err(src).kind, ParseErrorKind::Empty, "{src:?}");
    }
}

#[test]
fn a_call_has_its_brackets_and_a_name_without_them_is_a_reference() {
    assert_eq!(shape("PI()"), "(call PI)");
    assert_eq!(shape("PI"), "@PI");
}

#[test]
fn function_names_match_in_any_case_and_are_kept_as_written() {
    let names: Vec<String> = ["abs(-1)", "ABS(-1)", "Abs(-1)"]
        .iter()
        .map(|src| match ok(src) {
            Expr::Call(c) => {
                assert!(c.is("ABS") && c.is("abs") && !c.is("ABSX"));
                c.name.to_string()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(names, ["abs", "ABS", "Abs"]);
}

#[test]
fn a_name_nobody_knows_is_still_a_call() {
    assert_eq!(shape("NOT_A_REAL_NAME1(1,2)"), "(call NOT_A_REAL_NAME1 1 2)");
    // A leading underscore is outside MS-VSDX's catch-all rule and still read.
    assert_eq!(shape("_SHAPEMIN(1)"), "(call _SHAPEMIN 1)");
    assert_eq!(shape("_ELLIPSE_THETA(1,2,3)"), "(call _ELLIPSE_THETA 1 2 3)");
}

#[test]
fn an_empty_argument_slot_keeps_its_place() {
    assert_eq!(shape("BOUND(,0,FALSE,1,2)"), "(call BOUND ~ 0 FALSE 1 2)");
    assert_eq!(shape("F(1,,3)"), "(call F 1 ~ 3)");
    assert_eq!(shape("F(1,)"), "(call F 1 ~)");
    assert_eq!(shape("F(,)"), "(call F ~ ~)");
    assert_eq!(shape("F(,1)"), "(call F ~ 1)");
    // No slot at all is not one empty slot.
    assert_eq!(shape("F()"), "(call F)");
    assert_eq!(shape("F( )"), "(call F)");
    assert_eq!(shape("F( , )"), "(call F ~ ~)");
    // Functions-logic-errors: `IFERROR`'s second value may be missing after the comma; `AND(4>2,,3>2)`.
    assert_eq!(shape("IFERROR(7/0,)"), "(call IFERROR (/ 7 0) ~)");
    assert_eq!(shape("AND(4>2,,3>2)"), "(call AND (> 4 2) ~ (> 3 2))");
}

#[test]
fn a_sign_ranks_above_the_power() {
    // `-3^2` is 9, the sign applying to the base. Not the common reading, which gives -9.
    assert_eq!(shape("-3^2"), "(^ (neg 3) 2)");
    assert_eq!(shape("-(3^2)"), "(neg (^ 3 2))");
    assert_eq!(shape("2^-1"), "(^ 2 (neg 1))");
    // `-4^2` is 16; `0-4^2` is -16 because the binary minus ranks below the power; `2^-3`.
    assert_eq!(ok("-4^2"), bin(BinaryOp::Pow, neg(n(4.0)), n(2.0)));
    assert_eq!(shape("0-4^2"), "(- 0 (^ 4 2))");
    assert_eq!(shape("2^-3"), "(^ 2 (neg 3))");
    // The sign applies to a percent: `pct = atom { "%" }` is below `sign`.
    assert_eq!(shape("-3%"), "(neg (% 3))");
    assert_eq!(shape("-x^2*3"), "(* (^ (neg @x) 2) 3)");
}

#[test]
fn powers_and_equal_ranks_group_from_the_left() {
    // No rule makes `^` an exception.
    assert_eq!(shape("2^3^2"), "(^ (^ 2 3) 2)");
    assert_eq!(shape("2^(3^2)"), "(^ 2 (^ 3 2))");
    assert_eq!(shape("20/2/5"), "(/ (/ 20 2) 5)");
    assert_eq!(shape("90/3/6"), "(/ (/ 90 3) 6)");
    assert_eq!(shape("20-5-3"), "(- (- 20 5) 3)");
    assert_eq!(shape("2*3/4*5"), "(* (/ (* 2 3) 4) 5)");
    assert_eq!(shape("1<2<3"), "(< (< 1 2) 3)");
    assert_eq!(shape("1=2=3"), "(= (= 1 2) 3)");
    assert_eq!(shape("a&b&c"), "(& (& @a @b) @c)");
}

#[test]
fn a_product_binds_before_a_sum() {
    assert_eq!(shape("2+3*4"), "(+ 2 (* 3 4))");
    assert_eq!(shape("(2+3)*4"), "(* (+ 2 3) 4)");
}

#[test]
fn a_sum_binds_before_a_join() {
    assert_eq!(shape("3*4 & \" eggs\""), "(& (* 3 4) \" eggs\")");
    assert_eq!(shape("1+2 & \"x\""), "(& (+ 1 2) \"x\")");
    assert_eq!(shape("2 * 5 & \" cents\""), "(& (* 2 5) \" cents\")");
    assert_eq!(shape("1+2&3"), "(& (+ 1 2) 3)");
    assert_eq!(shape("\"sea\"&\"shell\"&\"s\""), "(& (& \"sea\" \"shell\") \"s\")");
}

#[test]
fn an_ordering_binds_before_an_equality() {
    assert_eq!(shape("1<2=TRUE"), "(= (< 1 2) TRUE)");
    assert_eq!(shape("TRUE=1<2"), "(= TRUE (< 1 2))");
    assert_eq!(shape("a<b<>c>d"), "(<> (< @a @b) (> @c @d))");
    // A join binds before an ordering, which binds before an equality.
    assert_eq!(shape("a&b<c"), "(< (& @a @b) @c)");
    assert_eq!(shape("a<b&c"), "(< @a (& @b @c))");
}

#[test]
fn a_percent_is_a_postfix_at_the_rank_of_the_signs() {
    assert_eq!(shape("40%"), "(% 40)");
    // `2^50%` is 2 to the power 0.5.
    assert_eq!(shape("2^50%"), "(^ 2 (% 50))");
    // `%` after a bracket, and not on the whole sum.
    assert_eq!(shape("(20+30)%"), "(% (+ 20 30))");
    assert_eq!(shape("2+50%"), "(+ 2 (% 50))");
    // Twice. R15 accepts it.
    assert_eq!(shape("200%%"), "(% (% 200))");
    assert_eq!(shape("3%^2"), "(^ (% 3) 2)");
    assert_eq!(shape("40 %"), "(% 40)");
    // After a reference and after a call (has 2 cells with a call).
    assert_eq!(shape("Width%"), "(% @Width)");
    assert_eq!(shape("F(1)%"), "(% (call F 1))");
    assert_eq!(shape("3 in/50%"), "(/ 3IN (% 50))");
}

#[test]
fn the_forms_of_a_plain_number() {
    for (src, value) in [(".5", 0.5), ("5.", 5.0), ("1.5E3", 1500.0), ("1.5e-1", 0.15), ("0", 0.0), ("007", 7.0), ("1E5", 1e5), ("1e+5", 1e5)] {
        assert_eq!(ok(src), n(value), "{src}");
    }
    assert_eq!(ok("4E-12"), n(4e-12));
    assert_eq!(ok("1E300"), n(1e300));
    // A number has no sign of its own in the tree: the sign is the operator.
    assert_eq!(ok("-5"), neg(n(5.0)));
    assert_eq!(ok("+5"), Expr::Unary(UnaryOp::Plus, Box::new(n(5.0))));
}

#[test]
fn boolean_words_in_any_case() {
    assert_eq!(ok("true"), Expr::Bool(true));
    assert_eq!(ok("TRUE"), Expr::Bool(true));
    assert_eq!(ok("True"), Expr::Bool(true));
    assert_eq!(ok("FALSE"), Expr::Bool(false));
    assert_eq!(ok("fAlSe"), Expr::Bool(false));
    // A longer name is a reference.
    assert_eq!(shape("TRUEX"), "@TRUEX");
    assert_eq!(shape("SUM(TRUE, TRUE, FALSE)"), "(call SUM TRUE TRUE FALSE)");
}

#[test]
fn the_exact_comparisons_in_symbols_and_in_words() {
    // Both spellings, the same rank as the tolerant ones. `5 GT 2` and `5 _GT_ 2` are one tree.
    assert_eq!(ok("5 GT 2"), ok("5 _GT_ 2"));
    assert_eq!(shape("5 GT 2"), "(_GT_ 5 2)");
    assert_eq!(shape("1 + 1 _EQ_ 2"), "(_EQ_ (+ 1 1) 2)");
    for (src, op) in [
        ("a _LT_ b", BinaryOp::ExactLt),
        ("a _LE_ b", BinaryOp::ExactLe),
        ("a _GT_ b", BinaryOp::ExactGt),
        ("a _GE_ b", BinaryOp::ExactGe),
        ("a _EQ_ b", BinaryOp::ExactEq),
        ("a _NE_ b", BinaryOp::ExactNe),
        ("a LT b", BinaryOp::ExactLt),
        ("a LE b", BinaryOp::ExactLe),
        ("a GT b", BinaryOp::ExactGt),
        ("a GE b", BinaryOp::ExactGe),
        ("a EQ b", BinaryOp::ExactEq),
        ("a NE b", BinaryOp::ExactNe),
        ("a gt b", BinaryOp::ExactGt),
        ("a _gt_ b", BinaryOp::ExactGt),
        ("a _Eq_ b", BinaryOp::ExactEq),
    ] {
        let Expr::Binary(got, ..) = ok(src) else { panic!("{src}") };
        assert_eq!(got, op, "{src}");
    }
    // The ranks: `_LT_` with `<` (7), `_EQ_` with `=` (8).
    assert_eq!(shape("a _EQ_ b _LT_ c"), "(_EQ_ @a (_LT_ @b @c))");
    assert_eq!(shape("a = b _LT_ c"), "(= @a (_LT_ @b @c))");
    assert_eq!(shape("a & b _GT_ c"), "(_GT_ (& @a @b) @c)");
    // And the others: the operand before a word is a number, a number with a unit, or a reference.
    assert_eq!(shape("3+5E-10 _EQ_ 3"), "(_EQ_ (+ 3 0.0000000005) 3)");
    assert_eq!(shape("1 ft _EQ_ 12 in"), "(_EQ_ 1FT 12IN)");
    assert_eq!(shape("5 EQ 2"), "(_EQ_ 5 2)");
    assert_eq!(shape("5 GE 2"), "(_GE_ 5 2)");
    assert_eq!(shape("5 LE 2"), "(_LE_ 5 2)");
    assert_eq!(shape("5 LT 2"), "(_LT_ 5 2)");
    assert_eq!(shape("5 NE 2"), "(_NE_ 5 2)");
    assert_eq!(shape("3 _GE_ 3+5E-10"), "(_GE_ 3 (+ 3 0.0000000005))");
    assert_eq!(shape("RAND() _GE_ 0"), "(_GE_ (call RAND) 0)");
    // A word is an operator only between operands: in front of a bracket it is a call, and glued to other letters it is a name.
    assert_eq!(shape("1 + GT(2)"), "(+ 1 (call GT 2))");
    assert_eq!(err("a GTX b").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn white_space_may_separate_tokens_and_may_be_left_out() {
    assert_eq!(ok("5*2"), ok("5 * 2"));
    assert_eq!(ok("5*2"), ok("  5\t*\n2  "));
    assert_eq!(ok("13in"), ok("13 in"));
    assert_eq!(ok("1in^100"), ok("1 in^100"));
    assert_eq!(ok("F(1,2)"), ok("F ( 1 , 2 )"));
    assert_eq!(ok("-(3)"), ok("- ( 3 )"));
    assert_eq!(ok("a<>b"), ok("a <> b"));
    assert_eq!(ok("a>=b"), ok("a >= b"));
}

#[test]
fn every_operator_symbol_and_its_rank() {
    // The table, and R13.
    let table = [
        ("a^b", BinaryOp::Pow, 3),
        ("a*b", BinaryOp::Mul, 4),
        ("a/b", BinaryOp::Div, 4),
        ("a+b", BinaryOp::Add, 5),
        ("a-b", BinaryOp::Sub, 5),
        ("a&b", BinaryOp::Cat, 6),
        ("a<b", BinaryOp::Lt, 7),
        ("a<=b", BinaryOp::Le, 7),
        ("a>b", BinaryOp::Gt, 7),
        ("a>=b", BinaryOp::Ge, 7),
        ("a=b", BinaryOp::Eq, 8),
        ("a<>b", BinaryOp::Ne, 8),
    ];
    // Glued to its operands a word is one name, not an operator.
    assert_eq!(shape("a_LT_b"), "@a_LT_b");
    for (src, op, rank) in table {
        let Expr::Binary(got, ..) = ok(src) else { panic!("{src}") };
        assert_eq!(got, op, "{src}");
        assert_eq!(op.rank(), rank, "{src}");
    }
    assert_eq!([BinaryOp::ExactLt, BinaryOp::ExactLe, BinaryOp::ExactGt, BinaryOp::ExactGe].map(BinaryOp::rank), [7; 4]);
    assert_eq!([BinaryOp::ExactEq, BinaryOp::ExactNe].map(BinaryOp::rank), [8; 2]);
    // Every pair of ranks: the tighter operator is the inner one, whichever side it is on.
    let by_rank = [("^", 3), ("*", 4), ("+", 5), ("&", 6), ("<", 7), ("=", 8)];
    for (i, (tight, _)) in by_rank.iter().enumerate() {
        for (loose, _) in &by_rank[i + 1..] {
            for src in [format!("a{tight}b{loose}c"), format!("a{loose}b{tight}c")] {
                let Expr::Binary(top, l, r) = ok(&src) else { panic!("{src}") };
                let top_is_loose = top.symbol() == *loose;
                assert!(top_is_loose, "{src}: the top operator is {}", top.symbol());
                let inner = if src.starts_with(&format!("a{tight}")) { &l } else { &r };
                assert!(matches!(**inner, Expr::Binary(op, ..) if op.symbol() == *tight), "{src}");
            }
        }
    }
}

#[test]
fn all_the_function_spellings_of_the_operators_are_calls() {
    // The table, all 20: no operator is rewritten as its call, and no call as its operator. The call is read like any other.
    let table = [
        ("_ADD(13, 29)", "_ADD", "+"),
        ("_CAT(\"x\", 5)", "_CAT", "&"),
        ("_DIV(6, 2 in)", "_DIV", "/"),
        ("_EEQ(1, 2)", "_EEQ", "_EQ_"),
        ("_EGE(1, 2)", "_EGE", "_GE_"),
        ("_EGT(1, 2)", "_EGT", "_GT_"),
        ("_ELE(1, 2)", "_ELE", "_LE_"),
        ("_ELT(1, 2)", "_ELT", "_LT_"),
        ("_ENE(1, 2)", "_ENE", "_NE_"),
        ("_FEQ(1, 2)", "_FEQ", "="),
        ("_FGE(1, 2)", "_FGE", ">="),
        ("_FGT(13, 29)", "_FGT", ">"),
        ("_FLE(1, 2)", "_FLE", "<="),
        ("_FLT(1, 2)", "_FLT", "<"),
        ("_FNE(1, 2)", "_FNE", "<>"),
        ("_MUL(1, 2)", "_MUL", "*"),
        ("_SUB(1, 2)", "_SUB", "-"),
        ("POW(1, 2)", "POW", "^"),
    ];
    let mut seen = 0;
    for (src, call, symbol) in table {
        let Expr::Call(c) = ok(src) else { panic!("{src}") };
        assert!(c.is(call) && c.args.len() == 2 && c.args.iter().all(Option::is_some), "{src}");
        // And the operator knows its function spelling, so `calc` can use one table for both.
        let Expr::Binary(op, ..) = ok(&format!("1 {symbol} 2")) else { panic!("{symbol}") };
        assert_eq!(op.function_name(), call, "{symbol}");
        assert_eq!(op.symbol(), symbol);
        seen += 1;
    }
    for (src, call) in [("_UMINUS(2 ft)", "_UMINUS"), ("_UPLUS(2 ft)", "_UPLUS"), ("_PCT(37)", PERCENT_FUNCTION)] {
        let Expr::Call(c) = ok(src) else { panic!("{src}") };
        assert!(c.is(call) && c.args.len() == 1, "{src}");
        seen += 1;
    }
    assert_eq!(seen, 21, "18 binary spellings, two signs and the percent: the 20, with POW");
    assert_eq!((UnaryOp::Minus.function_name(), UnaryOp::Plus.function_name()), ("_UMINUS", "_UPLUS"));
    assert_eq!((UnaryOp::Minus.symbol(), UnaryOp::Plus.symbol()), ("-", "+"));
}

#[test]
fn uminus_signs_stack() {
    assert_eq!(shape("2*-3"), "(* 2 (neg 3))");
    assert_eq!(shape("5--3"), "(- 5 (neg 3))");
    assert_eq!(shape("--5"), "(neg (neg 5))");
    assert_eq!(shape("+-5"), "(pos (neg 5))");
    assert_eq!(shape("-TRUE"), "(neg TRUE)");
    assert_eq!(shape("-\"3\""), "(neg \"3\")");
    assert_eq!(shape("-(3 in)"), "(neg 3IN)");
    assert_eq!(shape("-NA()"), "(neg (call NA))");
    assert_eq!(shape("a - -b"), "(- @a (neg @b))");
}

#[test]
fn brackets_leave_no_node_only_their_shape() {
    assert_eq!(ok("((1))"), ok("1"));
    assert_eq!(ok("(((1+2)))"), ok("1+2"));
    assert_eq!(ok("(1)+(2)"), ok("1+2"));
    assert_eq!(err("()").kind, ParseErrorKind::ExpectedOperand);
    assert_eq!(err("(1").kind, ParseErrorKind::ExpectedCloseParen);
    assert_eq!(err("1)").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("(1,2)").kind, ParseErrorKind::ExpectedCloseParen);
}

#[test]
fn syntax_errors_say_where() {
    for (src, kind, offset) in [
        ("1 +", ParseErrorKind::UnexpectedEnd, 3),
        ("1 + * 2", ParseErrorKind::ExpectedOperand, 4),
        ("1 2", ParseErrorKind::ExpectedOperator, 2),
        ("F(1 2)", ParseErrorKind::ExpectedArgumentSeparator, 4),
        ("F(1", ParseErrorKind::ExpectedArgumentSeparator, 3),
        ("F(", ParseErrorKind::UnexpectedEnd, 2),
        ("1 ? 2", ParseErrorKind::ExpectedOperator, 2),
        ("?", ParseErrorKind::ExpectedOperand, 0),
        ("1;2", ParseErrorKind::ExpectedOperator, 1),
        ("F(1;2)", ParseErrorKind::ExpectedArgumentSeparator, 3),
    ] {
        let e = err(src);
        assert_eq!((e.kind, e.offset), (kind, offset), "{src:?}: {e}");
        assert!(e.to_string().contains(&format!("byte {offset}")), "{e}");
    }
    // Each kind has its own name, for the error class a report counts.
    assert_eq!(ParseErrorKind::TooDeep.name(), "too_deep");
    assert_eq!(ParseErrorKind::ExpectedOperand.name(), "expected_operand");
}

#[test]
fn a_semicolon_is_not_a_separator() {
    // Files hold the universal form; the `;` is another reader's own and is not read.
    assert!(parse("F(1;2)").ast.is_err());
    assert!(parse("F(1,2)").ast.is_ok());
}

#[test]
fn blanks_may_be_left_out_around_the_underscored_comparisons() {
    // `_GT_` is spelled with its own underscores, so it ends where they do, whatever follows. Blanks go in either place, and
    // `1_GT_(2)` already read; `5_GT_2` did not, as the digit ran into the name.
    for (src, expect) in [
        ("5_GT_2", "(_GT_ 5 2)"),
        ("(1)_GT_2", "(_GT_ 1 2)"),
        ("1 _GT_2", "(_GT_ 1 2)"),
        ("1_GT_ 2", "(_GT_ 1 2)"),
        ("1_GT_(2)", "(_GT_ 1 2)"),
        ("1_GT_2", "(_GT_ 1 2)"),
        ("1_LT_2", "(_LT_ 1 2)"),
        ("1_LE_2", "(_LE_ 1 2)"),
        ("1_GE_2", "(_GE_ 1 2)"),
        ("1_EQ_2", "(_EQ_ 1 2)"),
        ("1_NE_2", "(_NE_ 1 2)"),
        ("1_gt_2", "(_GT_ 1 2)"),
        ("F(1)_EQ_2", "(_EQ_ (call F 1) 2)"),
        ("a _EQ_b", "(_EQ_ @a @b)"),
        ("a _EQ_b _NE_c", "(_NE_ (_EQ_ @a @b) @c)"),
        ("1 in_gt_2 in", "(_GT_ 1IN 2IN)"),
        ("\"a\"_EQ_\"b\"", "(_EQ_ \"a\" \"b\")"),
        ("-1_LT_-2", "(_LT_ (neg 1) (neg 2))"),
        ("1+1_EQ_2", "(_EQ_ (+ 1 1) 2)"),
    ] {
        assert_eq!(shape(src), expect, "{src}");
    }
    // A name that has the word inside it is a name: `a_GT_b` is one; the words without underscores need blanks.
    assert_eq!(shape("a_GT_b"), "@a_GT_b");
    assert_eq!(shape("_GT_x"), "@_GT_x");
    assert_eq!(err("5GT2").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("a GT2 b").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("a GTX b").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("1 _GT 2").kind, ParseErrorKind::ExpectedOperator);
}
