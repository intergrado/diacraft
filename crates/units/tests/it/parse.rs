//! `parse_unit`: the suffixes of R22, the `U` attribute values, and the spellings the other requirements add.

use diacraft_units::{Unit, parse_unit};

/// (text, unit): every suffix of the table in capitals, its `U` where that differs, and the extra spellings.
const SPELLINGS: &[(&str, Unit)] = &[
    ("IN", Unit::Inch),
    ("IN_F", Unit::InchFine),
    ("FT", Unit::Foot),
    ("YD", Unit::Yard),
    ("MI", Unit::Mile),
    ("MI_F", Unit::MileFine),
    ("NM", Unit::NauticalMile),
    ("MM", Unit::Millimetre),
    ("CM", Unit::Centimetre),
    ("M", Unit::Metre),
    ("KM", Unit::Kilometre),
    ("DL", Unit::DrawingDefault),
    ("DP", Unit::PageDefault),
    ("PT", Unit::Point),
    ("DT", Unit::TypeDefault),
    ("P", Unit::Pica),
    ("D", Unit::Didot),
    ("C", Unit::Cicero),
    ("DEG", Unit::Degree),
    ("RAD", Unit::Radian),
    ("DA", Unit::AngleDefault),
    ("ED", Unit::ElapsedDay),
    ("EH", Unit::ElapsedHour),
    ("EM", Unit::ElapsedMinute),
    ("ES", Unit::ElapsedSecond),
    ("EW", Unit::ElapsedWeek),
    ("DE", Unit::DurationDefault),
    ("%", Unit::Percent),
    ("PER", Unit::Percent),
    // Acres and hectares, `U` being AC and HA, and the longer spellings the units page accepts.
    ("ACRE", Unit::Acre),
    ("AC", Unit::Acre),
    ("ACRES", Unit::Acre),
    ("HA", Unit::Hectare),
    ("HA.", Unit::Hectare),
    ("HECTARE", Unit::Hectare),
    ("HECTARES", Unit::Hectare),
    // The compound units, as `U` writes them, and the degree sign of a literal.
    ("F_I", Unit::FeetInches),
    ("AD", Unit::DegreesMinutesSeconds),
    ("\u{b0}", Unit::DegreesMinutesSeconds),
    ("C_D", Unit::CiceroDidot),
    ("P_PT", Unit::PicaPoints),
    // R29, the same sentence: "the reference page names these display units FEET/INCH, the degree sign, CICERO/DIDOT and PICAPOINTS", and the parser accepts the union
    // of the spellings the sources show (How to read this file).
    ("FEET/INCH", Unit::FeetInches),
    ("CICERO/DIDOT", Unit::CiceroDidot),
    ("PICAPOINTS", Unit::PicaPoints),
    // DATE is a base of a multidimensional value, and the `U` of a date.
    ("DATE", Unit::Date),
    // The longer duration names and their short forms with a dot.
    ("EWEEK", Unit::ElapsedWeek),
    ("EDAY", Unit::ElapsedDay),
    ("EHOUR", Unit::ElapsedHour),
    ("EMINUTE", Unit::ElapsedMinute),
    ("ESECOND", Unit::ElapsedSecond),
    ("EW.", Unit::ElapsedWeek),
    ("ED.", Unit::ElapsedDay),
    ("EH.", Unit::ElapsedHour),
    ("EM.", Unit::ElapsedMinute),
    ("ES.", Unit::ElapsedSecond),
];

#[test]
fn every_spelling_of_the_spec_parses_to_its_unit() {
    for (text, unit) in SPELLINGS {
        assert_eq!(parse_unit(text), Some(*unit), "{text:?}");
    }
}

/// ABNF strings ignore letter case, so a suffix matches in any case (`13 in`, `Min(Width, Height)` style).
#[test]
fn a_suffix_matches_in_any_case() {
    for (text, unit) in SPELLINGS {
        assert_eq!(parse_unit(&text.to_lowercase()), Some(*unit), "{text:?} in lower case");
    }
    assert_eq!(parse_unit("In"), Some(Unit::Inch));
    assert_eq!(parse_unit("eWeek"), Some(Unit::ElapsedWeek));
    // White space around the unit (a unit typed after a number, `13 in`, is cut out of the text with its space) is no part of it.
    assert_eq!(parse_unit(" cm "), Some(Unit::Centimetre));
}

/// `code` is what a file writes in `U`, and parses back to the unit it came from.
#[test]
fn the_code_of_every_unit_parses_back() {
    for unit in Unit::ALL {
        if unit == Unit::Unitless {
            assert_eq!(unit.code(), "", "a number with no unit has no `U`");
            assert_eq!(parse_unit(""), None, "an empty text is not a unit");
        } else {
            assert_eq!(parse_unit(unit.code()), Some(unit), "{unit:?} has code {:?}", unit.code());
        }
    }
}

/// The `U` column: the codes a file holds, which differ from the suffix for percent (`PER`) and the acre (`AC`).
#[test]
fn the_u_attribute_values_of_the_spec() {
    let codes: &[(Unit, &str)] = &[
        (Unit::Inch, "IN"),
        (Unit::InchFine, "IN_F"),
        (Unit::Mile, "MI"),
        (Unit::MileFine, "MI_F"),
        (Unit::Percent, "PER"),
        (Unit::Acre, "AC"),
        (Unit::Hectare, "HA"),
        (Unit::FeetInches, "F_I"),
        (Unit::DegreesMinutesSeconds, "AD"),
        (Unit::CiceroDidot, "C_D"),
        (Unit::PicaPoints, "P_PT"),
        (Unit::Date, "DATE"),
        (Unit::DrawingDefault, "DL"),
    ];
    for (unit, code) in codes {
        assert_eq!(unit.code(), *code, "{unit:?}");
    }
}

/// The formula suffix, where a number can carry one: percent is `%` and the acre `ACRE`.
#[test]
fn the_formula_suffix_of_a_unit() {
    assert_eq!(Unit::Percent.suffix(), Some("%"));
    assert_eq!(Unit::Acre.suffix(), Some("ACRE"));
    assert_eq!(Unit::Hectare.suffix(), Some("HA"));
    assert_eq!(Unit::Inch.suffix(), Some("IN"));
    assert_eq!(Unit::InchFine.suffix(), Some("IN"), "`IN_F` only changes the ruler; the formula still says IN");
    assert_eq!(Unit::Unitless.suffix(), None);
    assert_eq!(Unit::FeetInches.suffix(), None, "a compound is written as two numbers, not with one suffix");
    for unit in Unit::ALL {
        if let Some(suffix) = unit.suffix() {
            assert!(parse_unit(suffix).is_some(), "{unit:?}: its own suffix {suffix:?} must parse");
        }
    }
}

/// What is not a single unit: powers, long names, words the spec does not list, text around the unit.
#[test]
fn text_that_is_not_a_unit_is_refused() {
    for text in [
        "",
        " ",
        "FOO",
        "INCH",
        "METRE",
        "IN^2",
        "SQ. IN.",
        "SQ IN",
        "IN.^2",
        "MULTIDIM",
        "FEET/",
        "FEET/INCHES",
        "CM CM",
        "1",
        "12IN",
        "IN,D",
        "[cm,D]",
        "\u{0}",
        "I\u{0}N",
        "\u{b0}\u{b0}",
        "%%",
    ] {
        assert_eq!(parse_unit(text), None, "{text:?}");
    }
    // Hostile input: a very long text and non-ASCII text give None and no panic.
    assert_eq!(parse_unit(&"IN".repeat(100_000)), None);
    assert_eq!(parse_unit("\u{1f600}"), None);
    assert_eq!(parse_unit("\u{130}N"), None, "a capital I with a dot lowers to two characters, and is not IN");
}

/// Every unit appears once in `ALL`, so the tables above cover every unit.
#[test]
fn all_lists_each_unit_once_and_the_spelling_table_reaches_every_unit() {
    let mut seen = std::collections::HashSet::new();
    for unit in Unit::ALL {
        assert!(seen.insert(unit), "{unit:?} is listed twice");
    }
    for unit in Unit::ALL {
        if unit != Unit::Unitless {
            assert!(SPELLINGS.iter().any(|(_, u)| *u == unit), "no spelling in the table parses to {unit:?}");
        }
    }
}
