//! Every vector of the batch, as data. A row is `(vector id, formula, expected value)`.
//!
//! `TEXTHEIGHT` and `TEXTWIDTH` measure with the text crate: their vectors are in `text_measure.rs`, and the ones about a cell that is
//! not `TheText` are here.

use std::sync::Arc;

use diacraft_calc::CellError::{Na, Value as Bad};
use diacraft_calc::{EvalCtx, Evaluator};
use diacraft_doc::{RowKey, TextPart};
use diacraft_testkit::build::ShapeBuilder;

use crate::{E, check_table, check_table_on, eval, eval_on, mismatch, probe};

/// The date of the spec's 7 March 2031 at 16:05:09.
const AT: &str = "(DATE(2031,3,7)+TIME(16,5,9))";
const DAY: &str = "DATE(2031,3,7)";

fn fmt(value: &str, picture: &str) -> String {
    format!("FORMAT({value},\"{picture}\")")
}

/// A table of `FORMAT` rows: `(id, value, picture, expected text)`.
fn check_formats(rows: &[(&str, &str, &str, &str)]) {
    let mut failures = Vec::new();
    for (id, value, picture, want) in rows {
        let f = fmt(value, picture);
        let v = eval(&f);
        match &v {
            diacraft_calc::Value::Text(t) if &**t == *want => {}
            other => failures.push(format!("{id}: `{f}`: got {other:?}, expected {want:?}")),
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

#[test]
fn all_vectors() {
    check_table(&[
        ("ALL", r#"LEN("Жук")"#, E::N(3.0)),
        ("ALL", r#"25 mm&"x""#, E::T("25MMx")),
        ("ALL", r#"3 in&"x""#, E::T("3\"x")),
        ("ALL", r#"2 ft&"x""#, E::T("2'x")),
        ("ALL", r#"1/3&"""#, E::T("0.333333333333333")),
    ]);
}

#[test]
fn all_the_locale_of_marks() {
    let with_language = |language: &str, formula: &str| {
        let (mut doc, at) = probe(|s| s, formula);
        Arc::make_mut(&mut doc.props).language = Some(Arc::from(language));
        let mut ctx = EvalCtx::default();
        Evaluator::new(&doc, &mut ctx).evaluate(&at)
    };
    assert_eq!(with_language("en-US", "FORMAT(1.5,\"0.00\")"), diacraft_calc::Value::text("1.50"));
    assert_eq!(with_language("de-DE", "FORMAT(1.5,\"0.00\")"), diacraft_calc::Value::text("1,50"));
    assert_eq!(with_language("de-DE", "DECIMALSEP()"), diacraft_calc::Value::text(","));
    assert_eq!(with_language("en-US", "DECIMALSEP()"), diacraft_calc::Value::text("."));
    // The grouping mark follows the decimal mark's locale.
    assert_eq!(with_language("de-DE", "FORMAT(1234.5,\"#,##0.0\")"), diacraft_calc::Value::text("1.234,5"));
    check_table(&[("ALL", r#"FORMATEX(1.5,"0.00",,,1031,0)"#, E::T("1,50")), ("DECIMALSEP", "DECIMALSEP()", E::T("."))]);
}

#[test]
fn picture_basics() {
    check_formats(&[
        ("PIC", "7.5", "", ""),
        ("PIC", "3 in", "esc(9)", "3.000 in."),
        ("PIC", DAY, "{<204>}", "2031-03-07"),
        ("PIC", "\"4.50\"", "0.0", "4.50"),
        ("PIC", "0.5", "#.00", ".50"),
        ("PIC", "12", "000", "012"),
        ("PIC", "1234567.5", "#,##0.0", "1,234,567.5"),
        ("PIC", "0", "#", ""),
        ("PIC", "0", "0", "0"),
    ]);
}

#[test]
fn a_date_under_a_numeric_picture_is_the_general_date() {
    check_formats(&[("PIC", AT, "esc(0)", "3/7/2031 4:05:09 PM")]);
}

#[test]
fn scientific_pictures() {
    check_formats(&[
        ("PIC", "1234.5", "0.00E+00", "1.23E+03"),
        ("PIC", "0.012", "0.0e-0", "1.2e-2"),
        ("PIC", "1234.5", "0.0e-0", "1.2e3"),
        ("PIC", "1234.5", "###.#e+#", "123.5e+1"),
    ]);
}

#[test]
fn rounding_is_half_away_from_zero_on_the_decimal_form() {
    check_formats(&[
        ("PIC", "4.015", "0.00", "4.02"),
        ("PIC", "0.285", "0.00", "0.29"),
        ("PIC", "2.5", "0", "3"),
        ("PIC", "-2.5", "0", "-3"),
        ("PIC", "0.25", "0.0", "0.3"),
    ]);
}

#[test]
fn a_value_that_rounds_to_zero_prints_scientific() {
    check_formats(&[
        ("PIC", "-0.2", "0", "0"),
        ("PIC", "0.04", "0.0", "4.E-2"),
        ("PIC", "-0.04", "0.0", "-4.E-2"),
        ("PIC", "0.0004", "0.00", "4.0E-4"),
        ("PIC", "0.0002", "0.000", "2.00E-4"),
        ("PIC", "0.0000999", "0.0", "1.0E-4"),
        ("PIC", "0.000999", "0.00", "1.00E-3"),
        ("PIC", "0.00096", "0.00", "9.6E-4"),
        ("PIC", "3E15", "0", "3000000000000000"),
        ("PIC", "3E15", "0.0", "3.0E15"),
        ("PIC", "1E15", "0.0", "1.0E15"),
        ("PIC", "999999999999999", "0.0", "999999999999999.0"),
        ("PIC", "3E15", "0.####", "3E15"),
        ("PIC", "12.34567", "esc(0)", "12.3457"),
        ("PIC", "7.5", "esc(0)", "7.5"),
        ("PIC", "7", "esc(0)", "7"),
        ("PIC", "0.00002468", "esc(0)", "2.468E-5"),
        ("PIC", "0.00003", "esc(0)", "3E-5"),
        ("PIC", "0.00006", "esc(0)", "0.0001"),
        ("PIC", "0.00002468", "esc(1)", "2.468e-5"),
        ("PIC", "0.00007 in", "0.0 u", "7.e-5 in."),
        ("PIC", "3E15", "0.0 u", "3.0e15"),
    ]);
}

#[test]
fn fractions() {
    check_formats(&[
        ("PIC", "2.4", "esc(15)", "2 2/5"),
        ("PIC", "-2.4", "esc(15)", "-2 2/5"),
        ("PIC", "0.31", "esc(15)", "1/3"),
        ("PIC", "0.31", "esc(17)", "22/71"),
        ("PIC", "0.75", "esc(17)", "3/4"),
        ("PIC", "0.02", "esc(15)", "0"),
        ("PIC", "0.999", "esc(15)", "1"),
        ("PIC", "5.37", "# #/4", "5 1/4"),
        ("PIC", "5.5", "# #/4", "5 2/4"),
        ("PIC", "3.25", "# #/#16", "3 1/4"),
        ("PIC", "3.2", "# #/#16", "3 3/16"),
        ("PIC", "3.2", "#/#", "16/5"),
    ]);
}

#[test]
fn unit_labels() {
    check_formats(&[
        ("PIC", "3 cm", "0 u", "3 cm."),
        ("PIC", "3 mm", "0 u", "3 mm."),
        ("PIC", "3 deg", "0 u", "3 deg."),
        ("PIC", "3 rad", "0 u", "3 rad."),
        ("PIC", "3 ft", "0 u", "3 ft."),
        ("PIC", "3 cm", "0.00 U", "3.00 CM."),
        ("PIC", "3 in", "0 uu", "3 inches"),
        ("PIC", "3 in", "0 UU", "3 INCHES"),
        ("PIC", "7", "0 u", "7"),
        ("PIC", "3 in", "esc(2)", "3"),
        ("PIC", "3 ft", "0", "3"),
        ("PIC", "3 mm", "0u", "3mm"),
        ("PIC", "3 in", "0u", "3in."),
    ]);
}

#[test]
fn text_pictures() {
    check_formats(&[
        ("PIC", "\"Mixed\"", "@ @+ @-", "Mixed MIXED mixed"),
        ("PIC", "\"ok\"", "'<'@'>'", "<ok>"),
        ("PIC", "\"ok\"", "\\[@\\]", "[ok]"),
        ("PIC", "\"Maß\"", "@+", "MAß"),
        ("PIC", "\"Dİk\"", "@-", "dik"),
    ]);
}

#[test]
fn date_pictures() {
    check_formats(&[
        ("PIC", AT, "M/d/yyyy", "3/7/2031"),
        ("PIC", AT, "dd/MM/yy", "07/03/31"),
        ("PIC", AT, "MMM d, yyyy", "Mar 7, 2031"),
        ("PIC", AT, "dddd", "Friday"),
        ("PIC", AT, "ddd", "Fri"),
        ("PIC", AT, "h:mm tt", "4:05 PM"),
        ("PIC", AT, "HH:mm:ss", "16:05:09"),
        ("PIC", AT, "h:mm:ss t", "4:05:09 P"),
        ("PIC", AT, "{{yyyy-MMM-dd h:mm am/pm}}", "2031-Mar-07 4:05 PM"),
        ("PIC", DAY, "c", "3/7/2031"),
        ("PIC", AT, "c", "3/7/2031 4:05:09 PM"),
        ("PIC", AT, "T", "4:05:09 PM"),
        ("PIC", "47569", "{{d-M-yyyy}}", "27-3-2030"),
        ("PIC", AT, "{<200>}", "3/7/2031"),
        ("PIC", AT, "{<202>}", "March 7, 2031"),
        ("PIC", "1.5", "[hh]:[mm]:[ss]", "36:00:00"),
        ("PIC", "0.75", "[h]", "18"),
        ("PIC", "2.25", "[hh]:[mm]\"\"m\"\"", "54:00m"),
    ]);
}

#[test]
fn casts() {
    check_table(&[
        ("PIC", r#"FORMATEX(5,"0.0 u","ft","in")"#, E::T("60.0 in.")),
        ("PIC", r#"FORMATEX(5 cm,"0.0 u","ft")"#, E::T("5.0 cm.")),
        ("PIC", r#"FORMATEX(2.54 cm,"0.0 u",,"in")"#, E::T("1.0 in.")),
        ("FORMATEX", r#"FORMATEX(5.5,"0.00 u","cm","ft")"#, E::T("0.18 ft.")),
        ("FORMATEX", r#"FORMATEX(1234.5,"0.00",,,1033,0)"#, E::T("1234.50")),
        ("FORMATEX", r#"FORMATEX(1234.5,"0.00",,,1031,0)"#, E::T("1234,50")),
        ("FORMATEX", r#"FORMATEX(1,"0.0","bz")"#, E::Err(Bad)),
        ("FORMAT", r#"FORMAT(1 cm/4,"0.000 u")"#, E::T("0.250 cm.")),
        ("FORMAT", r#"FORMAT(1 cm/4,"0.00 U")"#, E::T("0.25 CM.")),
        ("FORMAT", r#"FORMAT(1 cm/4,"0.0 u")"#, E::T("0.3 cm.")),
        // A cast that is not closed is `#VALUE!`.
        ("FORMAT", r#"FORMAT(1,"<,ft")"#, E::Err(Bad)),
        ("FORMAT", r#"FORMAT(NA(),"0.0")"#, E::Err(Na)),
        ("FORMAT", r#"FORMAT("x",NA())"#, E::Err(Na)),
    ]);
}

#[test]
fn field_pictures() {
    check_table(&[
        ("FIELDPICTURE", "FIELDPICTURE(14)", E::T("esc(14)")),
        ("FIELDPICTURE", "FIELDPICTURE(30)", E::T("T")),
        ("FIELDPICTURE", "FIELDPICTURE(27)", E::T("dd/MM/yy")),
        ("FIELDPICTURE", "FIELDPICTURE(205)", E::T("{<205>}")),
        ("FIELDPICTURE", "FORMAT(1.25,FIELDPICTURE(6))", E::T("1.25")),
    ]);
}

#[test]
fn characters() {
    check_table(&[
        ("CHAR", "CHAR(65)", E::T("A")),
        ("CHAR", "CHAR(10)", E::T("\n")),
        ("CHAR", "CHAR(128)", E::T("\u{20ac}")),
        ("CHAR", "CHAR(65.9)", E::T("A")),
        ("CHAR", "CHAR(255)", E::T("\u{ff}")),
        ("CHAR", "CHAR(159)", E::T("\u{178}")),
        ("CHAR", "CHAR(160)", E::T("\u{a0}")),
        ("CHAR", "CHAR(0)", E::Err(Bad)),
        ("CHAR", "CHAR(256)", E::Err(Bad)),
        ("UNICHAR", "UNICHAR(9731)", E::T("\u{2603}")),
        ("UNICHAR", "UNICHAR(65)", E::T("A")),
        ("UNICHAR", "UNICHAR(0)", E::Err(Bad)),
        ("UNICHAR", "UNICHAR(65536)", E::Err(Bad)),
        // 65537 wraps to 1.
        ("UNICHAR", "UNICHAR(65537)", E::T("\u{1}")),
    ]);
}

#[test]
fn find_left_right_mid() {
    check_table(&[
        ("FIND", r#"FIND("en","Glen")"#, E::N(3.0)),
        ("FIND", r#"FIND("EN","Glen",1,TRUE)"#, E::N(3.0)),
        ("FIND", r#"FIND("*","a*b")"#, E::N(2.0)),
        ("FIND", r#"FIND("n","Nan",3)"#, E::N(3.0)),
        ("FIND", r#"FIND("","Nan",2)"#, E::N(2.0)),
        ("FIND", r#"FIND("","Nan",9)"#, E::N(4.0)),
        ("FIND", r#"FIND("","")"#, E::N(1.0)),
        ("FIND", r#"FIND("EN","Glen")"#, E::Err(Bad)),
        ("FIND", r#"FIND("n","Nan",0)"#, E::Err(Bad)),
        ("FIND", r#"FIND("n","Nan",4)"#, E::Err(Bad)),
        ("LEFT", r#"LEFT("Glen",2)"#, E::T("Gl")),
        ("LEFT", r#"LEFT("Glen")"#, E::T("G")),
        ("LEFT", r#"LEFT("Glen",0)"#, E::T("")),
        ("LEFT", r#"LEFT("Glen",9)"#, E::T("Glen")),
        ("LEFT", r#"LEFT("Glen",-1)"#, E::T("Glen")),
        ("LEFT", r#"LEFT("Жук",2)"#, E::T("Жу")),
        ("LEFT", "LEFT(1234,2)", E::T("12")),
        ("RIGHT", r#"RIGHT("Glen",2)"#, E::T("en")),
        ("RIGHT", r#"RIGHT("Glen")"#, E::T("n")),
        ("RIGHT", r#"RIGHT("Glen",0)"#, E::T("")),
        ("RIGHT", r#"RIGHT("Glen",9)"#, E::T("Glen")),
        ("RIGHT", r#"RIGHT("Glen",-1)"#, E::T("Glen")),
        ("RIGHT", "RIGHT(1234,2)", E::T("34")),
        ("MID", r#"MID("Glenfinnan",5,4)"#, E::T("finn")),
        ("MID", r#"MID("Glen",3,9)"#, E::T("en")),
        ("MID", r#"MID("Glen",3,-1)"#, E::T("en")),
        ("MID", r#"MID("Жук",2,1)"#, E::T("у")),
        ("MID", r#"MID("Glen",0,2)"#, E::Err(Bad)),
        ("MID", r#"MID("Glen",-1,2)"#, E::T("")),
        ("MID", r#"MID("Glen",9,2)"#, E::T("")),
        ("LEN", r#"LEN("Glen")"#, E::N(4.0)),
        ("LEN", r#"LEN("")"#, E::N(0.0)),
        ("LEN", r#"LEN("Жук")"#, E::N(3.0)),
        ("LEN", "LEN(12)", E::N(2.0)),
        ("LEN", "LEN(TRUE)", E::N(4.0)),
    ]);
}

#[test]
fn lengths_count_utf16_units() {
    // The project rule: a character above U+FFFF is two units; a cut between its halves is U+FFFD and never a crash.
    check_table(&[("ALL", "LEN(UNICHAR(55357)&UNICHAR(56832))", E::N(2.0)), ("ALL", "LEFT(UNICHAR(55357)&UNICHAR(56832),1)", E::T("\u{fffd}"))]);
}

#[test]
fn lists() {
    check_table(&[
        ("INDEX", r#"INDEX(1,"ash;elm;;oak")"#, E::T("elm")),
        ("INDEX", r#"INDEX(2,"ash;elm;;oak")"#, E::T("")),
        ("INDEX", r#"INDEX(0,";ash")"#, E::T("")),
        ("INDEX", r#"INDEX(0,"ash;elm")"#, E::T("ash")),
        ("INDEX", r#"INDEX(9,"ash;elm")"#, E::T("")),
        ("INDEX", r#"INDEX(9,"ash;elm",,"none")"#, E::T("none")),
        ("INDEX", r#"INDEX(-1,"ash;elm",,"none")"#, E::T("none")),
        ("INDEX", r#"INDEX(9,"ash",,NA())"#, E::Err(Na)),
        ("INDEX", r#"INDEX(1,"ash; elm ")"#, E::T(" elm ")),
        ("INDEX", r#"INDEX(1,"ash | elm"," | ")"#, E::T("elm")),
        ("INDEX", r#"INDEX(1.9,"ash;elm;oak")"#, E::T("elm")),
        ("INDEX", r#"INDEX("1","ash;elm")"#, E::T("elm")),
        // `position` is a `vSignedInt`: 32767 is the last number it holds, and 32768 is not valid.
        ("INDEX", r#"INDEX(32767,"ash;elm")"#, E::T("")),
        ("INDEX", r#"INDEX(32768,"ash;elm")"#, E::Err(Bad)),
        ("LOOKUP", r#"LOOKUP("elm","ash;elm;;oak")"#, E::N(1.0)),
        ("LOOKUP", r#"LOOKUP("","ash;;oak")"#, E::N(1.0)),
        ("LOOKUP", r#"LOOKUP("fir","ash;elm")"#, E::N(-1.0)),
        ("LOOKUP", r#"LOOKUP(3,"1;2;3")"#, E::N(2.0)),
        ("LOOKUP", r#"LOOKUP("ELM","ash;elm")"#, E::N(1.0)),
        ("LOOKUP", r#"LOOKUP("a;b","a;b;c")"#, E::N(-1.0)),
        ("LOOKUP", r#"LOOKUP("o","wood","o")"#, E::N(-1.0)),
        ("LOOKUP", r#"LOOKUP("elm"," ash ; elm ")"#, E::N(1.0)),
        ("LOOKUP", r#"LOOKUP(" elm ","ash; elm ")"#, E::N(1.0)),
    ]);
}

#[test]
fn case_and_equality() {
    check_table(&[
        ("LOWER", r#"LOWER("MiXeD")"#, E::T("mixed")),
        ("LOWER", "LOWER(TRUE)", E::T("true")),
        ("UPPER", r#"UPPER("MiXeD")"#, E::T("MIXED")),
        ("UPPER", r#"UPPER("épée")"#, E::T("ÉPÉE")),
        ("STRSAME", r#"STRSAME("Oak","oak")"#, E::B(false)),
        ("STRSAME", r#"STRSAME("Oak","oak",TRUE)"#, E::B(true)),
        ("STRSAME", r#"STRSAME("Oak","Oak ")"#, E::B(false)),
        ("STRSAME", r#"STRSAME("é","É",TRUE)"#, E::B(true)),
        ("STRSAME", r#"STRSAME("e","É",TRUE)"#, E::B(false)),
        ("STRSAME", r#"STRSAME(12,"12")"#, E::B(true)),
        ("STRSAME", r#"STRSAME(NA(),"x")"#, E::B(false)),
        ("STRSAMEEX", r#"STRSAMEEX("Oak","oak",1033,1)"#, E::B(true)),
        ("STRSAMEEX", r#"STRSAMEEX("Oak","oak",1033,0)"#, E::B(false)),
        ("STRSAMEEX", r#"STRSAMEEX("a","a",99999,0)"#, E::B(false)),
    ]);
}

#[test]
fn editing_strings() {
    check_table(&[
        ("REPLACE", r#"REPLACE("2031-03",6,2,"11")"#, E::T("2031-11")),
        ("REPLACE", r#"REPLACE("abcd",2,0,"z")"#, E::T("azbcd")),
        ("REPLACE", r#"REPLACE("abc",0,1,"z")"#, E::T("abcz")),
        ("REPLACE", r#"REPLACE("abc",9,1,"z")"#, E::T("abcz")),
        ("REPLACE", r#"REPLACE("abcd",3,9,"z")"#, E::T("abz")),
        ("REPLACE", r#"REPLACE("abcd",3,-1,"z")"#, E::T("abz")),
        ("REPT", r#"REPT("ab",3)"#, E::T("ababab")),
        ("REPT", r#"REPT("ab",0)"#, E::T("")),
        ("REPT", r#"REPT("ab",2.9)"#, E::T("abab")),
        ("REPT", r#"REPT("ab",-1)"#, E::Err(Bad)),
        ("REWIDEN", r#"REWIDEN("a","b","text")"#, E::T("text")),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c","-","+")"#, E::T("a+b+c")),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c","-","+",2)"#, E::T("a-b+c")),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c-d","-","+",2)"#, E::T("a-b+c-d")),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c","-","+",0)"#, E::Err(Bad)),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c","-","+",9)"#, E::T("a-b-c")),
        ("SUBSTITUTE", r#"SUBSTITUTE("a-b-c","-","+",-1)"#, E::T("a-b-c")),
        ("SUBSTITUTE", r#"SUBSTITUTE("aXbx","x","+")"#, E::T("aXb+")),
        ("SUBSTITUTE", r#"SUBSTITUTE("aXbx","x","+",,TRUE)"#, E::T("a+b+")),
        ("SUBSTITUTE", r#"SUBSTITUTE("abc","","+")"#, E::T("abc")),
        ("TRIM", r#"TRIM("  red   oak ")"#, E::T("red oak")),
        ("TRIM", r#"TRIM("")"#, E::T("")),
        ("TRIM", r#"TRIM("a"&CHAR(9)&CHAR(9)&"b")"#, E::T("a b")),
        ("TRIM", "TRIM(UNICHAR(160)&UNICHAR(160))", E::T("")),
        ("LISTSEP", "LISTSEP()", E::T(",")),
    ]);
}

#[test]
fn a_formula_built_with_listsep_parses() {
    let v = eval(r#""MAX(1"&LISTSEP()&"2)""#);
    let diacraft_calc::Value::Text(t) = v else { panic!("{v:?}") };
    assert!(diacraft_formula::parse(&t).ast.is_ok(), "{t}");
}

#[test]
fn a_huge_repeat_is_capped_and_never_exhausts_memory() {
    // The result is capped; project rule `#VALUE!` beyond the cap.
    check_table(&[("REPT", r#"REPT("abcdefghij",100000000)"#, E::Err(Bad)), ("REPT", r#"LEN(REPT("a",1000))"#, E::N(1000.0))]);
}

#[test]
fn shape_text() {
    let on = |text: &'static str| move |s: ShapeBuilder| s.text(text);
    check_table_on(on("Oak\n"), &[("SHAPETEXT", "SHAPETEXT(TheText)", E::T("Oak"))]);
    check_table_on(on("\n"), &[("SHAPETEXT", "SHAPETEXT(TheText)", E::T("")), ("SHAPETEXT", r#"SHAPETEXT(TheText)="""#, E::B(true))]);
    check_table_on(
        on("Ash\nElm\n"),
        &[("SHAPETEXT", "SHAPETEXT(TheText)", E::T("Ash\r\nElm")), ("SHAPETEXT", "SHAPETEXT(TheText,16)", E::T("Ash Elm"))],
    );
    check_table_on(on("a\tb"), &[("SHAPETEXT", "SHAPETEXT(TheText,4)", E::T("a b"))]);
    check_table_on(on("a  \tb"), &[("SHAPETEXT", "SHAPETEXT(TheText,64)", E::T("a b"))]);
    // Text with no final feed keeps what it has.
    check_table_on(on("Oak"), &[("SHAPETEXT", "SHAPETEXT(TheText)", E::T("Oak"))]);
    // A cell other than `TheText` gives empty text.
    check_table_on(on("Oak"), &[("SHAPETEXT", "SHAPETEXT(Width)", E::T(""))]);
}

#[test]
fn a_field_in_the_text_is_unsupported_unless_fields_are_left_out() {
    // Asks for a field's own row through its own picture (`FIG`). That expansion is not worked (the round, I5): the
    // call is unsupported and the saved value stands, where it was once `Kite` or empty. With flag 2 the fields are left out, and the rest
    // is the text (`ab`).
    let with_field = |s: ShapeBuilder| {
        s.text_parts(vec![TextPart::Chars(Arc::from("a")), TextPart::Fld { ix: 0, text: Arc::from("Kite") }, TextPart::Chars(Arc::from("b\n"))])
            .row_cell("Field", None, RowKey::Ix(0), "Value", "Fig")
            .row_cell("Field", None, RowKey::Ix(0), "Format", "@+")
    };
    let v = eval_on(with_field, "SHAPETEXT(TheText)");
    assert!(matches!(&v, diacraft_calc::Value::Unsupported(n) if &**n == "SHAPETEXT"), "{v:?}");
    check_table_on(with_field, &[("SHAPETEXT", "SHAPETEXT(TheText,2)", E::T("ab"))]);
    let v = eval_on(with_field, "EVALTEXT(TheText)");
    assert!(matches!(&v, diacraft_calc::Value::Unsupported(n) if &**n == "EVALTEXT"), "{v:?}");
    // The same for a call inside another function: the unknown is not turned into a value.
    let v = eval_on(with_field, "LEN(SHAPETEXT(TheText))");
    assert!(matches!(v, diacraft_calc::Value::Unsupported(_)), "{v:?}");
}

#[test]
fn a_format_over_a_length_in_the_default_unit_is_unsupported() {
    // A width with no `U` prints in the document's unit, which this crate does not look up; it once printed inches.
    let v = eval_on(|s| s.cell("Width", "2"), r#"FORMAT(Width,"0.00 u")"#);
    assert!(matches!(&v, diacraft_calc::Value::Unsupported(n) if &**n == "FORMAT"), "{v:?}");
    // A cast names the unit, so it is worked.
    let v = eval_on(|s| s.cell("Width", "2"), r#"FORMATEX(Width,"0.00 u",,"in")"#);
    assert_eq!(mismatch(&v, &E::T("2.00 in.")), None, "{v:?}");
    // A bare number, a number with a unit written, and a date are worked as before.
    check_table(&[("FORMAT", r#"FORMAT(2,"0.00")"#, E::T("2.00")), ("FORMAT", r#"FORMAT(2 in,"0.00 u")"#, E::T("2.00 in."))]);
}

#[test]
fn text_measures() {
    check_table(&[("TEXTHEIGHT", "TEXTHEIGHT(Width,4 in)", E::Err(Bad)), ("TEXTWIDTH", "TEXTWIDTH(Height)", E::Err(Bad))]);
    // A measure of real text is a number, in the faces the evaluator is given or the bundled ones (`text_measure.rs` holds the numbers).
    let v = eval_on(|s| s.text("Oak"), "TEXTWIDTH(TheText)");
    assert!(matches!(v, diacraft_calc::Value::Number(_)), "{v:?}");
}
