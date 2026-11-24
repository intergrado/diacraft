//! A cell's value: the file's text is the truth, and what is read from it is derived on each read, so the two cannot
//! disagree.

use diacraft_doc::{Reading, Value, ValueKind};

#[test]
fn the_text_comes_back_exactly_as_given() {
    // `0.10` is not reprinted as `0.1`, `1.#INF` stays, and so do spellings of the theme marker.
    for text in ["0.10", "1.#INF", "-1.#INF", "1E-05", "  3 ", "", "Themed", "themed", "#FF00AA", "two words", "\u{e9}\n\t", "+7", "007"] {
        assert_eq!(Value::from_text(text).text(), text);
    }
}

#[test]
fn it_reads_numbers_from_the_text_each_time() {
    for (text, expected) in [("0.10", 0.1), ("-3", -3.0), ("1E-5", 1e-5), ("1e3", 1000.0), (".5", 0.5), ("5.", 5.0), ("+2.5", 2.5), ("007", 7.0)] {
        assert_eq!(Value::from_text(text).reading(), Reading::Number(expected), "{text}");
        assert_eq!(Value::from_text(text).number(), Some(expected), "{text}");
    }
}

#[test]
fn text_that_rust_would_parse_but_is_not_a_number_is_not_one() {
    // `inf`, `nan` and `infinity` parse as f64 in Rust; in a file they are words, and a NaN must never get in.
    for text in ["inf", "-inf", "Infinity", "nan", "NaN", "1_000", "0x10", "1e", "e5", "--1", "1.2.3", " 1", "1 ", "\u{661}", "1e999", "-1e999"] {
        let v = Value::from_text(text);
        assert_eq!(v.number(), None, "{text:?}");
        assert!(!matches!(v.reading(), Reading::Number(_)), "{text:?} read as {:?}", v.reading());
    }
}

#[test]
fn the_other_readings() {
    assert_eq!(Value::from_text("").reading(), Reading::Empty);
    assert_eq!(Value::from_text("1.#INF").reading(), Reading::Infinite { negative: false });
    assert_eq!(Value::from_text("-1.#INF").reading(), Reading::Infinite { negative: true });
    // Compared ignoring case.
    for text in ["Themed", "themed", "THEMED"] {
        assert_eq!(Value::from_text(text).reading(), Reading::Themed, "{text}");
        assert!(Value::from_text(text).is_themed());
    }
    assert_eq!(Value::from_text("#FF0000").reading(), Reading::Text);
    assert_eq!(Value::from_text("TRUE").reading(), Reading::Text);
    assert_eq!(Value::from_text("Themed ").reading(), Reading::Text, "the marker is the whole text");
}

#[test]
fn a_number_made_by_calculation_prints_so_that_it_reads_back_the_same() {
    for n in [0.0, -0.0, 1.0, 0.1, 1.0 / 3.0, 1e-7, 1e21, 123_456_789.123_456_79, f64::MAX, f64::MIN_POSITIVE, -2.5e-300] {
        let v = Value::from_number(n).unwrap();
        assert_eq!(v.number(), Some(n), "{n:e} printed as {}", v.text());
    }
    // A value too large for a double is written 1.#INF; a NaN has no value.
    assert_eq!(Value::from_number(f64::INFINITY).unwrap().text(), "1.#INF");
    assert_eq!(Value::from_number(f64::NEG_INFINITY).unwrap().text(), "-1.#INF");
    assert_eq!(Value::from_number(f64::NAN), None);
}

#[test]
fn an_evaluation_that_computes_the_same_number_keeps_the_files_text() {
    // `1.50` stays `1.50` when evaluation computes 1.5.
    let v = Value::from_text("1.50");
    let kept = v.with_number(1.5);
    assert_eq!(kept.text(), "1.50");
    let changed = v.with_number(2.0);
    assert_eq!(changed.text(), "2");
    // A text value is replaced by the number.
    assert_eq!(Value::from_text("abc").with_number(3.0).text(), "3");
    // A NaN keeps what was there rather than writing one.
    assert_eq!(v.with_number(f64::NAN).text(), "1.50");
}

#[test]
fn a_cell_with_no_value_takes_a_default_from_its_unit() {
    use ValueKind::*;
    for (unit, kind) in [
        ("MM", Length),
        ("IN", Length),
        ("IN_F", Length),
        ("FT", Length),
        ("F_I", Length),
        ("P_PT", Length),
        ("C_D", Length),
        ("DT", Length),
        ("DL", Length),
        ("AC", Length),
        ("HA", Length),
        ("DEG", Angle),
        ("RAD", Angle),
        ("AD", Angle),
        ("DA", Angle),
        ("ED", Days),
        ("ES", Days),
        ("DE", Days),
        ("BOOL", Scalar),
        ("CY", Scalar),
        ("DATE", Scalar),
        ("PER", Scalar),
        ("COLOR", Color),
        ("PNT", Point),
        ("STR", Text),
    ] {
        assert_eq!(ValueKind::from_unit(unit), Some(kind), "{unit}");
    }
    // Units the spec does not list, and no unit at all, fall back to the cell's own default.
    for unit in ["", "NURBS", "POLYLINE", "GUID", "NUM", "MULTIDIM", "XYZZY"] {
        assert_eq!(ValueKind::from_unit(unit), None, "{unit:?}");
    }
}

#[test]
fn an_evaluation_of_a_themed_value_leaves_the_marker_and_its_spelling() {
    // `Themed` with a Theme style that evaluates to 0.03 reads 0.03 in memory, and the file
    // goes on saying `Themed`, in the case it was written in. `with_number` is what a commit of an evaluation calls.
    for spelling in ["Themed", "themed", "THEMED"] {
        let v = Value::from_text(spelling);
        let after = v.with_number(0.03);
        assert_eq!(after.text(), spelling);
        assert!(after.is_themed());
    }
    // A number that is not themed is replaced, as before, and a NaN changes nothing in either.
    assert_eq!(Value::from_text("1").with_number(0.03).text(), "0.03");
    assert_eq!(Value::from_text("themed").with_number(f64::NAN).text(), "themed");
}

#[test]
fn a_zero_keeps_its_sign_through_an_evaluation() {
    // `0` and `-0` read as equal numbers, and are not the same text: an evaluation that computes -0 does not leave `0` standing.
    assert_eq!(Value::from_text("0").with_number(-0.0).text(), "-0");
    assert_eq!(Value::from_text("-0").with_number(0.0).text(), "0");
    assert_eq!(Value::from_text("0").with_number(0.0).text(), "0", "the same zero keeps the file's text");
    assert_eq!(Value::from_text("0.0").with_number(0.0).text(), "0.0");
    assert_eq!(Value::from_text("-0.00").with_number(-0.0).text(), "-0.00");
}
