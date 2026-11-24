//! The currency codes: the table, its lookups, and the rule that a currency literal takes a code
//! of the table and no other three letters (the table half).
//!
//! The table's check against the text of the spec itself is in a test file of its own, which reads the spec by a path from the crate's
//! folder.

use diacraft_formula::{CURRENCIES, Currency, Expr, Number, ParseErrorKind, currency_by_code, currency_by_id, parse, print};
use diacraft_units::{Unit, parse_unit};

use crate::{err, ok, shape};

// ---- the table's own shape, from the spec's prose

#[test]
fn cy_the_ids_and_the_codes_the_prose_names() {
    assert_eq!(CURRENCIES.len(), 192);
    // ID 0 is the system's currency, ID 1 the code of three X, which shows no symbol.
    assert_eq!(currency_by_id(0).map(|c| c.code), Some("SYS"));
    assert_eq!(currency_by_id(1).map(|c| c.code), Some("XXX"));
    // IDs 2 to 9 and 51 to 55 are in neither table, and MS-VSDX's -1 has no code.
    for id in (2..=9).chain(51..=55).chain([-1, -2, 205, 206, 1000, i32::MAX, i32::MIN]) {
        assert_eq!(currency_by_id(id), None, "{id}");
    }
    // The historic codes stay valid.
    for code in ["PLZ", "RUR", "XEU", "YUN", "BRR", "BYB", "BGL", "LVL", "MGF", "TJR", "CSD"] {
        assert!(currency_by_code(code).is_some(), "{code}");
    }
    // The first and last IDs of the two runs of the table.
    assert_eq!(currency_by_id(10).map(|c| c.code), Some("EUR"));
    assert_eq!(currency_by_id(50).map(|c| c.code), Some("ZAR"));
    assert_eq!(currency_by_id(56).map(|c| c.code), Some("ARS"));
    assert_eq!(currency_by_id(204).map(|c| c.code), Some("CSD"));
}

#[test]
fn the_table_is_in_id_order_with_distinct_ids_and_codes_of_three_capitals() {
    assert!(CURRENCIES.windows(2).all(|w| w[0].id < w[1].id), "IDs rise, so a lookup by ID may search");
    let mut codes: Vec<&str> = CURRENCIES.iter().map(|c| c.code).collect();
    assert!(codes.iter().all(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 192, "no code is listed twice");
}

#[test]
fn lookups_by_id_and_by_code_find_each_entry_and_agree() {
    for (n, c) in CURRENCIES.iter().enumerate() {
        assert_eq!(currency_by_code(c.code), Some(c), "{} by code", c.code);
        assert_eq!(currency_by_id(c.id), Some(c), "{} by ID", c.id);
        // The reference is to the table's own entry.
        assert!(std::ptr::eq(currency_by_id(c.id).unwrap(), &CURRENCIES[n]));
        assert!(std::ptr::eq(currency_by_code(c.code).unwrap(), &CURRENCIES[n]));
    }
    // Each ID that is not listed finds nothing, whichever side of an entry it falls.
    let listed: Vec<i32> = CURRENCIES.iter().map(|c| c.id).collect();
    for id in -3..=210 {
        assert_eq!(currency_by_id(id).is_some(), listed.contains(&id), "{id}");
    }
}

#[test]
fn a_code_is_matched_whole_and_in_capitals() {
    // All capitals; a literal does not ignore case, and the lookup is the literal's rule. A string that ignores case
    // folds it before it asks.
    for text in [
        "",
        " ",
        "E",
        "EU",
        "EURO",
        "EUR ",
        " EUR",
        "EUR\n",
        "eur",
        "Eur",
        "eUR",
        "EuR",
        "ZZZ",
        "XYZ",
        "ABC",
        "USA",
        "EU\u{0}",
        "ÉUR",
        "EU\u{212A}",
        "ＥＵＲ",
        "10",
        "EU1",
    ] {
        assert_eq!(currency_by_code(text), None, "{text:?}");
    }
    assert_eq!(currency_by_code(&"eur".to_ascii_uppercase()).map(|c| c.id), Some(10));
    // Every code of the table, in any other case, is not a match.
    for c in &CURRENCIES {
        assert_eq!(currency_by_code(&c.code.to_ascii_lowercase()), None, "{}", c.code);
        let mixed: String = c.code.chars().enumerate().map(|(i, ch)| if i == 1 { ch.to_ascii_lowercase() } else { ch }).collect();
        assert_eq!(currency_by_code(&mixed), None, "{mixed}");
    }
}

#[test]
fn ids_126_and_127_follow_ms_vsdx_and_not_the_page() {
    // Table half: `CY(1,126)` is `1.0000ERN` by MS-VSDX, where the page gives `TPE`. Task 6 reads `V` through this lookup.
    assert_eq!(currency_by_id(126), Some(&Currency { id: 126, code: "ERN" }));
    assert_eq!(currency_by_id(127), Some(&Currency { id: 127, code: "TPE" }));
    assert_eq!(currency_by_code("ERN").map(|c| c.id), Some(126));
    assert_eq!(currency_by_code("TPE").map(|c| c.id), Some(127));
    // The neighbours, so a shift of the table by one place is seen.
    assert_eq!(currency_by_id(125).map(|c| c.code), Some("DJF"));
    assert_eq!(currency_by_id(128).map(|c| c.code), Some("EEK"));
}

// ----: the literal

#[test]
fn cy_twelve_point_five_euro_is_a_currency() {
    assert_eq!(ok("12.5EUR"), Expr::Currency { value: 12.5, code: "EUR".into() });
    assert_eq!(shape("12.5EUR"), "12.5EUR");
}

#[test]
fn cy_twelve_point_five_zzz_is_no_currency_literal() {
    // Any other three letters are treated as any unknown name: here a name follows a number, which is no operator.
    let e = err("12.5ZZZ");
    assert_eq!(e.kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(e.offset, 4, "at the letters");
    for text in ["12.5XYZ", "5ABC", "5USA", "1e3ZZZ", "0.5QQQ", "2*5ZZZ", "SUM(5ZZZ, 3)", "F(1, 5ZZZ)"] {
        assert!(parse(text).ast.is_err(), "{text}");
    }
    // The same letters as a name of their own are a cell reference and no currency.
    assert_eq!(shape("ZZZ"), "@ZZZ");
    assert_eq!(shape("5+ZZZ"), "(+ 5 @ZZZ)");
}

#[test]
fn cy_twelve_point_five_deg_is_an_angle_because_no_code_is_a_unit() {
    assert_eq!(ok("12.5DEG"), Expr::Number(Number::Measured { value: 12.5, unit: Unit::Degree, power: None }));
    assert_eq!(ok("5 DEG"), Expr::Number(Number::Measured { value: 5.0, unit: Unit::Degree, power: None }));
    assert_eq!(ok("5RAD"), Expr::Number(Number::Measured { value: 5.0, unit: Unit::Radian, power: None }));
}

#[test]
fn cy_no_code_of_the_table_is_a_unit_spelling() {
    // The spec checked it against the suffixes; the crate reads every spelling `parse_unit` knows, in any case, so ask that.
    for c in &CURRENCIES {
        assert_eq!(parse_unit(c.code), None, "{} is a unit", c.code);
        assert_eq!(parse_unit(&format!("{}.", c.code)), None, "{}. is a unit", c.code);
    }
}

#[test]
fn cy_every_code_of_the_table_is_a_literal_right_after_a_number() {
    for c in &CURRENCIES {
        let want = Expr::Currency { value: 12.5, code: c.code.into() };
        assert_eq!(ok(&format!("12.5{}", c.code)), want, "{}", c.code);
        assert_eq!(ok(&format!("=12.5{}", c.code)), want, "{}", c.code);
        // Among other tokens, a code ends where the letters end.
        assert_eq!(shape(&format!("SUM(1,2{},3)", c.code)), format!("(call SUM 1 2{} 3)", c.code));
        assert_eq!(shape(&format!("2*3{}+1", c.code)), format!("(+ (* 2 3{}) 1)", c.code));
        assert_eq!(shape(&format!("5{} _GT_ 2", c.code)), format!("(_GT_ 5{} 2)", c.code));
    }
    // ID 1's code is one of them, and so is ID 0's.
    assert_eq!(shape("5XXX"), "5XXX");
    assert_eq!(shape("5SYS"), "5SYS");
}

#[test]
fn cy_the_number_may_be_any_literal_the_grammar_gives_before_a_code() {
    assert_eq!(ok("12USD"), Expr::Currency { value: 12.0, code: "USD".into() });
    assert_eq!(ok(".5USD"), Expr::Currency { value: 0.5, code: "USD".into() });
    assert_eq!(ok("5.USD"), Expr::Currency { value: 5.0, code: "USD".into() });
    assert_eq!(ok("1.5e3USD"), Expr::Currency { value: 1500.0, code: "USD".into() });
    assert_eq!(ok("1E3EUR"), Expr::Currency { value: 1000.0, code: "EUR".into() });
    // A code that starts with the letter E, after a number that could take an exponent, is still a code.
    assert_eq!(ok("5EUR"), Expr::Currency { value: 5.0, code: "EUR".into() });
    assert_eq!(ok("5EEK"), Expr::Currency { value: 5.0, code: "EEK".into() });
    assert_eq!(ok("5E3EEK"), Expr::Currency { value: 5000.0, code: "EEK".into() });
}

#[test]
fn cy_a_blank_may_stand_between_the_number_and_the_code_and_it_reads_the_same() {
    // Does not forbid a blank, R20 says "followed by", a unit takes a blank, and
    // writes `5 USD`. A blank of any kind, one or several, reads as the same currency as none.
    for c in &CURRENCIES {
        let want = Expr::Currency { value: 12.5, code: c.code.into() };
        for sep in ["", " ", "  ", "\t", "\n", " \t \n "] {
            let text = format!("12.5{sep}{}", c.code);
            assert_eq!(ok(&text), want, "{text:?}");
            // The tree keeps no blank, so it prints with none (the form `V` holds) and that reads back as the same tree.
            assert_eq!(print(&ok(&text)), format!("12.5{}", c.code), "{text:?}");
            assert_eq!(parse(&print(&ok(&text))).ast, Ok(want.clone()), "{text:?}");
        }
        // In an argument list, and before an operator word.
        assert_eq!(shape(&format!("SUM(5 {}, 3)", c.code)), format!("(call SUM 5{} 3)", c.code));
        assert_eq!(shape(&format!("5 {} _GT_ 2", c.code)), format!("(_GT_ 5{} 2)", c.code));
        // A number that could take an exponent still takes the blank and the code.
        assert_eq!(ok(&format!("1E3 {}", c.code)), Expr::Currency { value: 1000.0, code: c.code.into() });
    }
    // A blank does not make a code of other letters or of other case: the letters are a name after a number, which is no operator,
    // with a blank as without one.
    for text in ["5 usd", "5 Usd", "5 eur", "5 ZZZ", "5 XYZ", "5 ABC", "5 EURO", "5 EU", "5  zzz", "5\tUSDX", "1E3 ZZZ"] {
        assert_eq!(err(text).kind, ParseErrorKind::ExpectedOperator, "{text:?}");
    }
    assert_eq!(err("SUM(5 ZZZ, 3)").kind, ParseErrorKind::ExpectedArgumentSeparator);
    assert_eq!(err("SUM(5 usd, 3)").kind, ParseErrorKind::ExpectedArgumentSeparator);
    // A unit after a blank is still a unit, and not a currency.
    assert_eq!(ok("5 DEG"), Expr::Number(Number::Measured { value: 5.0, unit: Unit::Degree, power: None }));
}

#[test]
fn cy_a_code_in_a_literal_is_in_capitals_only() {
    // The project rule: a literal does not ignore case, because its letters decide how the formula parses.
    for c in &CURRENCIES {
        for text in
            [c.code.to_ascii_lowercase(), c.code.chars().enumerate().map(|(i, ch)| if i == 0 { ch } else { ch.to_ascii_lowercase() }).collect()]
        {
            assert!(parse(&format!("12.5{text}")).ast.is_err(), "12.5{text}");
        }
    }
    for text in ["5 eur", "5eur", "5Eur", "5gbp", "5usd", "12.5xxx", "5sys"] {
        assert!(parse(text).ast.is_err(), "{text}");
    }
}

#[test]
fn cy_the_letters_are_taken_whole() {
    // A code is the whole run of letters: a longer or shorter run is not one, and a digit or a point after it is not part of it.
    for text in ["12.5EURO", "12.5EU", "12.5E", "12.5USDX", "12.5XUSD", "12.5USDA", "12.5EURUSD", "5EUR2", "5EUR.", "5EUR_X", "5EURé"] {
        assert!(parse(text).ast.is_err(), "{text}");
    }
    // A run of three letters that is a code stands for it even when more follows after an operator.
    assert_eq!(shape("5USD+3EUR"), "(+ 5USD 3EUR)");
}

#[test]
fn cy_leaves_the_other_forms_after_a_number_as_they_were() {
    // Nothing else the lexer reads after a number is changed: units, powers, feet and inches, ciceros and picas.
    assert_eq!(shape("5 IN"), "5IN");
    assert_eq!(shape("5 in"), "5IN");
    assert_eq!(shape("2C4"), "2C4");
    assert_eq!(shape("3P6"), "3P6");
    assert_eq!(shape("8FT 4IN"), "8FT4IN");
    assert_eq!(shape("3 DATE^2"), "3DATE^2");
    assert_eq!(shape("3 SQ. IN."), "3IN^2");
    assert_eq!(shape("5 1/2"), "5.5");
    // A three-letter run that is neither a unit nor a code is no literal, as the two-letter and four-letter ones never were.
    assert_eq!(err("5 AB").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("5 ABCD").kind, ParseErrorKind::ExpectedOperator);
    assert_eq!(err("5 ABC").kind, ParseErrorKind::ExpectedOperator);
}

#[test]
fn a_currency_prints_right_after_its_number_and_reads_back() {
    for c in &CURRENCIES {
        let tree = ok(&format!("12.5{}", c.code));
        let text = print(&tree);
        assert_eq!(text, format!("12.5{}", c.code));
        assert_eq!(parse(&text).ast, Ok(tree));
    }
    // A number that prints with an exponent still reads back with its code.
    for (value, text) in [(1e21, "1e21EUR"), (5e-324, "5e-324EUR"), (1e16, "1e16EEK")] {
        let tree = Expr::Currency { value, code: text.get(text.len() - 3..).unwrap().into() };
        assert_eq!(print(&tree), text);
        assert_eq!(parse(text).ast, Ok(tree));
    }
}
