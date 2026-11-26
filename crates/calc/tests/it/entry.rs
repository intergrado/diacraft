//! A cell named by the text of its reference (`cell_address`), the reference a cell is spelled with (`reference_name`), and a formula
//! tried on a sheet before it is written (`evaluate_text`).
//!
//! The vectors are, written as tables of `(vector id, reference, address)`. A
//! refusal is held to its words, not only to being a refusal, since the commands put those words in front of a person. The costs are counts (the steps
//! an evaluation took, `Evaluated::steps`), never a time; the one real-time check is `#[ignore]`d.

use std::time::Instant;

use diacraft_calc::{
    Budget, CellError, EvalCtx, EvalOptions, EvalResult, Evaluated, Evaluator, MAX_REFERENCE_BYTES, RefError, Value, cell_address, evaluate_text,
    reference_name,
};
use diacraft_doc::{CellAddr, Document, Owner, RowKey, ShapeRef, SheetRef, listed_cells};
use diacraft_formula::{ParseError, ParseErrorKind};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use diacraft_units::{Quantity, Unit};
use proptest::prelude::*;

// --- cell_address ---

fn ix(section: &str, geometry: Option<u32>, row: u32, cell: &str) -> CellAddr {
    CellAddr::in_row(section, geometry, RowKey::Ix(row), cell)
}

fn named(section: &str, row: &str, cell: &str) -> CellAddr {
    CellAddr::in_row(section, None, RowKey::name(row), cell)
}

#[test]
fn a_reference_names_the_cell_the_specs_vectors_give() {
    let rows: Vec<(&str, &str, CellAddr)> = vec![
        // The table.
        ("R35", "Height", CellAddr::direct("Height")),
        ("V24", "Char.Size[2]", ix("Character", None, 1, "Size")),
        ("V24", "Char.Size", ix("Character", None, 0, "Size")),
        ("V25", "Scratch.B3", ix("Scratch", None, 2, "B")),
        ("V25", "Geometry2.X4", ix("Geometry", Some(1), 4, "X")),
        ("V26", "User.Count", named("User", "Count", "Value")),
        ("V26", "Actions.Go", named("Actions", "Go", "Menu")),
        ("V26", "Controls.Handle", named("Control", "Handle", "X")),
        ("V26", "Hyperlink.Site", named("Hyperlink", "Site", "Description")),
        ("R35", "Prop.Cost.Label", named("Property", "Cost", "Label")),
        // The task's own vectors.
        ("T1", "Geometry1.X2", ix("Geometry", Some(0), 2, "X")),
        ("T1", "Prop.Cost", named("Property", "Cost", "Value")),
        // The flags of a Geometry section are cells of the section itself, with no row.
        ("R40", "Geometry1.NoFill", CellAddr::in_section("Geometry", Some(0), "NoFill")),
        ("R40", "Geometry3.NoShow", CellAddr::in_section("Geometry", Some(2), "NoShow")),
        // The spellings of the table.
        ("R40", "SmartTags.Tag1", named("ActionTag", "Tag1", "X")),
        ("R40", "Fields.A[1]", ix("Field", None, 0, "A")),
        ("R40", "FillGradientStops.GradientStopColor[3]", ix("FillGradient", None, 2, "GradientStopColor")),
        ("R40", "Layers.Visible[2]", ix("Layer", None, 1, "Visible")),
        ("R40", "Para.IndFirst[1]", ix("Paragraph", None, 0, "IndFirst")),
        ("R40", "Connections.X3", ix("Connection", None, 2, "X")),
        // A section's own file name is a name too, and an extension section is its own name.
        ("R40", "Character.Size[2]", ix("Character", None, 1, "Size")),
        ("R40", "Extra.Cell", named("Extra", "Cell", "Value")),
        // A name in single quotes, and in white space.
        ("E3", "'Width'", CellAddr::direct("Width")),
        ("E3", "'My cell'", CellAddr::direct("My cell")),
        ("E3", "  Width  ", CellAddr::direct("Width")),
        ("E3", "Prop.'Unit price'.Value", named("Property", "Unit price", "Value")),
    ];
    let mut failures = Vec::new();
    for (id, text, expected) in &rows {
        match cell_address(text) {
            Ok(got) if got == *expected => {}
            other => failures.push(format!("{id}: `{text}`: got {other:?}, expected {expected:?}")),
        }
    }
    assert!(failures.is_empty(), "{} of {} vectors failed:\n{}", failures.len(), rows.len(), failures.join("\n"));
}

#[test]
fn a_sheet_before_the_reference_is_refused_in_words() {
    for text in [
        "Sheet.4!Width",
        "Pages[Site-2]!Sheet.9!EndY",
        "ThePage!User.Origin",
        "TheDoc!User.Count",
        "Styles!Callout!LineWeight",
        "LISTSHEETREF()!Width",
    ] {
        let e = cell_address(text).unwrap_err();
        assert_eq!(e, RefError::Qualified, "{text}");
        assert_eq!(e.to_string(), "name the sheet with id, page, master, style or document, not in the reference", "{text}");
    }
}

#[test]
fn a_text_that_is_not_one_reference_is_refused_in_words() {
    for text in ["1+2", "Width+1", "SUM(1,2)", "3", "3 in", "TRUE", "\"Width\"", "Width Height", "Sheet.4!SHEETREF()", "#ff0000"] {
        let e = cell_address(text).unwrap_err();
        assert!(matches!(e, RefError::NotOneReference | RefError::Parse(_)), "{text}: {e:?}");
    }
    assert_eq!(
        cell_address("1+2").unwrap_err().to_string(),
        "not one cell reference: give a reference such as Width, Geometry1.X2 or Prop.Cost.Label"
    );
    assert_eq!(cell_address("TRUE").unwrap_err(), RefError::NotOneReference);
    assert_eq!(cell_address("SUM(1,2)").unwrap_err(), RefError::NotOneReference);
}

#[test]
fn a_text_that_does_not_parse_gives_the_parse_error_with_its_place() {
    let e = cell_address("Prop.").unwrap_err();
    assert_eq!(e, RefError::Parse(ParseError { kind: ParseErrorKind::BadName, offset: 5 }));
    assert_eq!(e.to_string(), "not a cell reference: a name was expected (at byte 5)");
    let e = cell_address("Width+").unwrap_err();
    assert!(matches!(e, RefError::Parse(_) | RefError::NotOneReference), "{e:?}");
}

#[test]
fn nothing_and_a_leading_equals_sign_are_refused_in_words() {
    for text in ["", "   ", "\t\n"] {
        let e = cell_address(text).unwrap_err();
        assert_eq!(e, RefError::Empty, "{text:?}");
        assert_eq!(e.to_string(), "name the cell by its reference, such as Width or Geometry1.X2");
    }
    let e = cell_address("=Width").unwrap_err();
    assert_eq!(e, RefError::LeadingEquals);
    assert_eq!(e.to_string(), "a cell is named by its reference without a leading =: write Width, not =Width");
    assert_eq!(cell_address("  = Width").unwrap_err(), RefError::LeadingEquals);
}

#[test]
fn a_form_no_cell_has_is_refused_with_the_reason() {
    let rows: &[(&str, RefError, &str)] = &[
        ("Width[2]", RefError::CellTakesNoRow, "a cell directly under the sheet takes no [row]"),
        ("Char[2].Size", RefError::SectionTakesNoRow, "a section's name takes no [row]: the row goes after the cell, as in Char.Size[2]"),
        ("Geometry.X1", RefError::GeometryNumber, "a Geometry section is named with its number, from 1: Geometry1"),
        ("Geometry0.X1", RefError::GeometryNumber, "a Geometry section is named with its number, from 1: Geometry1"),
        ("GeometryFoo.X1", RefError::GeometryNumber, "a Geometry section is named with its number, from 1: Geometry1"),
        ("Scratch.A0", RefError::RowZero, "row numbers start at 1"),
        ("Char.Size[0]", RefError::RowZero, "row numbers start at 1"),
        ("Scratch.A[x]", RefError::RowNotANumber, "a row of an indexed section is a whole number: Scratch.A3 or Char.Size[2]"),
        ("Prop.Cost[2]", RefError::NamedRowTakesNoRow, "a named row takes no [index]: Prop.Cost.Label"),
        ("Prop.Cost.Label[2]", RefError::NamedRowTakesNoRow, "a named row takes no [index]: Prop.Cost.Label"),
        ("Prop.Cost.Label.More", RefError::TooManyParts, "a reference has at most a section, a row and a cell: Prop.Cost.Label"),
        ("Scratch.A4294967296", RefError::NumberTooLarge, "a number in the reference is larger than 4294967295"),
        ("Geometry4294967296.X1", RefError::NumberTooLarge, "a number in the reference is larger than 4294967295"),
    ];
    for (text, error, words) in rows {
        let e = cell_address(text).unwrap_err();
        assert_eq!(&e, error, "{text}");
        assert_eq!(e.to_string(), *words, "{text}");
    }
}

#[test]
fn a_row_number_too_large_to_hold_is_no_other_row() {
    // `Scratch.A4294967296` was read as `Scratch.A`, row 1, because the digits that did not fit were dropped (a wrong cell for a write to land on).
    assert_eq!(cell_address("Scratch.A4294967295").unwrap(), ix("Scratch", None, 4_294_967_294, "A"));
    assert_eq!(cell_address("Geometry1.X4294967295").unwrap(), ix("Geometry", Some(0), 4_294_967_295, "X"));
    assert_eq!(cell_address("Scratch.A99999999999").unwrap_err(), RefError::NumberTooLarge);
    assert_eq!(cell_address("Geometry1.X99999999999").unwrap_err(), RefError::NumberTooLarge);
    // In brackets it is not a number at all, and is refused as any row that is no number.
    assert_eq!(cell_address("Char.Size[4294967296]").unwrap_err(), RefError::RowNotANumber);
}

#[test]
fn a_formula_that_reads_a_row_number_too_large_to_hold_reads_no_row() {
    let shape = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "5");
    let (doc, at) = crate::probe(|_| shape, "Scratch.A4294967296");
    let v = Evaluator::new(&doc, &mut EvalCtx::default()).evaluate(&at);
    assert_eq!(v, Value::Error(CellError::Ref), "it is not row 1, which holds 5");
}

#[test]
fn a_reference_is_at_most_eight_kibibytes_and_a_name_at_most_a_kibibyte() {
    assert_eq!(MAX_REFERENCE_BYTES, 8_192);
    let at_the_limit = format!("Width{}", " ".repeat(MAX_REFERENCE_BYTES - 5));
    assert_eq!(cell_address(&at_the_limit).unwrap(), CellAddr::direct("Width"));
    let over = format!("Width{}", " ".repeat(MAX_REFERENCE_BYTES - 4));
    let e = cell_address(&over).unwrap_err();
    assert_eq!(e, RefError::TooLong);
    assert_eq!(e.to_string(), "a cell reference is at most 8192 bytes long");
    let name = "a".repeat(1024);
    assert_eq!(cell_address(&name).unwrap(), CellAddr::direct(&name));
    assert!(matches!(cell_address(&"a".repeat(1025)).unwrap_err(), RefError::Parse(ParseError { kind: ParseErrorKind::BadName, .. })));
}

// --- reference_name ---

#[test]
fn a_cell_is_spelled_as_a_formula_names_it() {
    let rows: Vec<(CellAddr, &str)> = vec![
        (CellAddr::direct("PinX"), "PinX"),
        (ix("Geometry", Some(0), 2, "X"), "Geometry1.X2"),
        (ix("Geometry", Some(4), 17, "B"), "Geometry5.B17"),
        (CellAddr::in_section("Geometry", Some(0), "NoFill"), "Geometry1.NoFill"),
        (ix("Scratch", None, 0, "A"), "Scratch.A1"),
        (ix("Scratch", None, 2, "B"), "Scratch.B3"),
        (ix("Character", None, 1, "Size"), "Char.Size[2]"),
        (ix("Character", None, 0, "Size"), "Char.Size[1]"),
        (ix("Connection", None, 2, "X"), "Connections.X3"),
        (ix("Paragraph", None, 0, "IndFirst"), "Para.IndFirst[1]"),
        (ix("FillGradient", None, 2, "GradientStopColor"), "FillGradientStops.GradientStopColor[3]"),
        (named("Property", "Cost", "Value"), "Prop.Cost.Value"),
        (named("Property", "Cost", "Label"), "Prop.Cost.Label"),
        (named("User", "Total", "Prompt"), "User.Total.Prompt"),
        (named("Actions", "Go", "Menu"), "Actions.Go.Menu"),
        (named("Control", "Handle", "X"), "Controls.Handle.X"),
        (named("ActionTag", "Tag1", "X"), "SmartTags.Tag1.X"),
        (named("Hyperlink", "Site", "Description"), "Hyperlink.Site.Description"),
        (named("Extra", "Cell", "Value"), "Extra.Cell.Value"),
        // A name that is no plain name is quoted, and a quote in it is doubled.
        (CellAddr::direct("My cell"), "'My cell'"),
        (CellAddr::direct("it's"), "'it''s'"),
        (CellAddr::direct("3"), "'3'"),
        (CellAddr::direct("TRUE"), "'TRUE'"),
        (named("Property", "Unit price", "Value"), "Prop.'Unit price'.Value"),
        (ix("Scratch", None, 0, "A b"), "Scratch.'A b'[1]"),
    ];
    for (addr, text) in &rows {
        assert_eq!(reference_name(addr).as_deref(), Some(*text), "{addr:?}");
        assert_eq!(cell_address(text).as_ref(), Ok(addr), "{text}");
    }
}

#[test]
fn a_cell_with_no_reference_form_has_no_reference() {
    let name_too_long = "a".repeat(1025);
    let rows: Vec<(&str, CellAddr)> = vec![
        ("no name", CellAddr::direct("")),
        ("a name of 1,025 bytes", CellAddr::direct(&name_too_long)),
        ("an unnamed row of a named section", ix("User", None, 0, "Value")),
        ("an unnamed row of a Prop section", ix("Property", None, 3, "Label")),
        ("a Geometry section with no number", CellAddr::in_section("Geometry", None, "NoFill")),
        ("a Geometry row with no section number", ix("Geometry", None, 2, "X")),
        ("a flag of a section that is not Geometry", CellAddr::in_section("Scratch", None, "A")),
        ("a row with no section", CellAddr { section: None, row: Some(RowKey::Ix(1)), cell: "X".into() }),
        ("a section number past what a reference holds", ix("Geometry", Some(u32::MAX), 1, "X")),
        ("a row number past what a reference holds", ix("Scratch", None, u32::MAX, "A")),
        ("a number on a section that has none", ix("Scratch", Some(3), 1, "A")),
        ("a section another spelling takes", named("Prop", "Cost", "Value")),
        ("an empty row name", named("User", "", "Value")),
        ("a section of no name", named("", "Cost", "Value")),
    ];
    for (why, addr) in &rows {
        assert_eq!(reference_name(addr), None, "{why}: {addr:?}");
    }
}

/// The addresses a listed cell may sit at: in every kind of row its section may hold, and for a Geometry section also as one of its own cells.
fn places_of(section: Option<&str>, cell: &str) -> Vec<CellAddr> {
    let Some(section) = section else { return vec![CellAddr::direct(cell)] };
    let geometry = (section == "Geometry").then_some(0);
    let mut places = vec![
        CellAddr::in_row(section, geometry, RowKey::Ix(0), cell),
        CellAddr::in_row(section, geometry, RowKey::Ix(6), cell),
        CellAddr::in_row(section, geometry, RowKey::name("Row_1"), cell),
    ];
    if geometry.is_some() {
        places.push(CellAddr::in_section(section, geometry, cell));
    }
    places
}

#[test]
fn every_listed_cell_has_a_reference_and_it_maps_back() {
    let mut cells = 0;
    let mut spelled = 0;
    for (section, cell, _) in listed_cells() {
        cells += 1;
        let mut any = false;
        for addr in places_of(section, cell) {
            if let Some(text) = reference_name(&addr) {
                any = true;
                assert_eq!(cell_address(&text).as_ref(), Ok(&addr), "{section:?} {cell}: `{text}`");
            }
        }
        assert!(any, "no place of {section:?} {cell} has a reference");
        spelled += 1;
    }
    assert!(cells >= 300, "the catalogue lists {cells} cells");
    assert_eq!(cells, spelled);
}

// --- the two as properties ---

/// A name of the odd kinds a file or a person may give: plain, with a space or a dot or a quote, a number, a word, a unicode letter.
fn name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Za-z_][A-Za-z0-9_]{0,12}",
        "[A-Za-z]",
        "[0-9]{1,3}",
        Just("TRUE".to_owned()),
        Just("false".to_owned()),
        Just("My cell".to_owned()),
        Just("it's".to_owned()),
        Just("a.b".to_owned()),
        Just("é1".to_owned()),
        Just("Width[2]".to_owned()),
        Just(String::new()),
        "[ -~]{0,6}",
    ]
}

fn section_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        proptest::sample::select(diacraft_doc::SECTION_NAMES.to_vec()).prop_map(str::to_owned),
        Just("Prop".to_owned()),
        Just("Char".to_owned()),
        Just("Geometry5".to_owned()),
        Just("Extra".to_owned()),
        name_strategy(),
    ]
}

fn addr_strategy() -> impl Strategy<Value = CellAddr> {
    let row = prop_oneof![
        Just(None),
        any::<u32>().prop_map(|n| Some(RowKey::Ix(n % 40))),
        Just(Some(RowKey::Ix(u32::MAX))),
        name_strategy().prop_map(|n| Some(RowKey::name(&n))),
    ];
    let geometry = prop_oneof![Just(None), Just(Some(0)), Just(Some(3)), Just(Some(u32::MAX))];
    (proptest::option::of(section_strategy()), geometry, row, name_strategy()).prop_map(|(section, geometry, row, cell)| CellAddr {
        section: section.map(|s| diacraft_doc::SectionKey::new(&s, geometry)),
        row,
        cell: cell.as_str().into(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    /// A reference that is given maps back to the cell it was given for, and a cell that has none says so: never a reference to another cell.
    #[test]
    fn a_reference_is_never_given_for_another_cell(addr in addr_strategy()) {
        if let Some(text) = reference_name(&addr) {
            prop_assert_eq!(text.trim(), text.as_str());
            prop_assert_eq!(cell_address(&text), Ok(addr));
        }
    }
}

/// A text that looks like a reference: one of the forms of R35 filled from a pool of names and numbers, and often with one character put in at a place,
/// so that about half of them read and the others are near misses.
fn reference_like() -> impl Strategy<Value = String> {
    let pool = prop_oneof![
        Just("Width"),
        Just("Geometry1"),
        Just("Geometry3"),
        Just("Scratch"),
        Just("Prop"),
        Just("Char"),
        Just("User"),
        Just("Connections"),
        Just("Extra"),
        Just("X"),
        Just("A"),
        Just("Size"),
        Just("Cost"),
        Just("'My cell'"),
        Just("'it''s'"),
        Just("Value"),
    ];
    let number = prop_oneof![Just(0u32), 1..40u32, Just(u32::MAX)];
    (0..8usize, pool.clone(), pool.clone(), pool, number, proptest::option::of((0..30usize, "[ -~]"))).prop_map(|(form, a, b, c, n, noise)| {
        let mut text = match form {
            0 => a.to_owned(),
            1 => format!("{a}.{b}"),
            2 => format!("{a}.{b}{n}"),
            3 => format!("{a}.{b}[{n}]"),
            4 => format!("{a}.{b}.{c}"),
            5 => format!("Geometry{n}.{b}{n}"),
            6 => format!("{a}!{b}"),
            _ => format!(" {a}.{b} "),
        };
        if let Some((at, ch)) = noise {
            let at = at.min(text.len());
            if text.is_char_boundary(at) {
                text.insert_str(at, &ch);
            }
        }
        text
    })
}

#[test]
fn a_text_that_is_read_is_spelled_back() {
    // The fuzz target's property over drawn text: a text that is read has a spelling, which reads as the same cell. Run by hand through the strategy so
    // that the run counts what it held: a property over texts that never read would hold nothing.
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let strategy = reference_like();
    let (mut read, mut refused) = (0, 0);
    for _ in 0..6_000 {
        let text = strategy.new_tree(&mut runner).unwrap().current();
        match cell_address(&text) {
            Ok(addr) => {
                read += 1;
                let spelled = reference_name(&addr).unwrap_or_else(|| panic!("`{text}` reads as {addr:?} and has no reference"));
                assert_eq!(cell_address(&spelled), Ok(addr), "`{text}` spelled `{spelled}`");
            }
            Err(e) => {
                refused += 1;
                assert!(!e.to_string().is_empty());
            }
        }
    }
    assert!(read >= 1_500 && refused >= 1_500, "{read} read, {refused} refused");
}

#[test]
fn a_sweep_of_cells_is_spelled_or_refused_and_never_spelled_wrong() {
    // Every combination of a few sections, rows and cells, the odd ones among them: some have a reference and some have none, and the sweep holds
    // that both happen (a property test over drawn cells that all came out as `None` would hold nothing).
    let sections = [
        None,
        Some(("User", None)),
        Some(("Property", None)),
        Some(("Scratch", None)),
        Some(("Geometry", Some(0))),
        Some(("Geometry", None)),
        Some(("Prop", None)),
        Some(("", None)),
    ];
    let rows = [
        None,
        Some(RowKey::Ix(0)),
        Some(RowKey::Ix(5)),
        Some(RowKey::Ix(u32::MAX)),
        Some(RowKey::name("Row_1")),
        Some(RowKey::name("My row")),
        Some(RowKey::name("")),
    ];
    let cells = ["X", "A", "Size", "My cell", "TRUE", "7", "", "é", "a'b"];
    let (mut spelled, mut refused) = (0, 0);
    for section in &sections {
        for row in &rows {
            for cell in cells {
                let addr =
                    CellAddr { section: section.map(|(name, ix)| diacraft_doc::SectionKey::new(name, ix)), row: row.clone(), cell: cell.into() };
                match reference_name(&addr) {
                    Some(text) => {
                        spelled += 1;
                        assert_eq!(cell_address(&text), Ok(addr), "`{text}`");
                    }
                    None => refused += 1,
                }
            }
        }
    }
    assert!(spelled >= 100 && refused >= 100, "{spelled} spelled, {refused} refused of {}", sections.len() * rows.len() * cells.len());
}

// --- hostile references ---

#[test]
fn a_hostile_reference_is_refused_or_read_and_never_stops_the_run() {
    let long = |piece: &str| piece.repeat(MAX_REFERENCE_BYTES / piece.len().max(1));
    let inputs: Vec<(&str, String)> = vec![
        ("quotes", long("'")),
        ("doubled quotes", format!("'{}'", long("''"))),
        ("dots", long(".")),
        ("a name and a dot", long("a.")),
        ("opening brackets", long("[")),
        ("an index not closed", format!("Char.Size[{}", "9".repeat(MAX_REFERENCE_BYTES - 12))),
        ("qualifiers", long("a!")),
        ("brackets of a call", long("(")),
        ("a name of 8 KiB", "a".repeat(MAX_REFERENCE_BYTES)),
        ("digits", "9".repeat(MAX_REFERENCE_BYTES)),
        ("a megabyte", "a.".repeat(1 << 19)),
        ("white space", " ".repeat(MAX_REFERENCE_BYTES)),
        ("equals signs", "=".repeat(MAX_REFERENCE_BYTES)),
        ("a section, a row and a cell, each a kibibyte and a quote", format!("{}.{}.{}'", "s".repeat(1024), "r".repeat(1024), "c".repeat(1024))),
    ];
    for (why, text) in &inputs {
        match cell_address(text) {
            Ok(addr) => {
                let spelled = reference_name(&addr);
                assert!(spelled.is_some(), "{why}: read as {addr:?} and has no reference");
                assert_eq!(cell_address(&spelled.unwrap_or_default()), Ok(addr), "{why}");
            }
            Err(e) => assert!(!e.to_string().is_empty(), "{why}"),
        }
    }
    // The longest honest reference: three names of a kibibyte each, the second all quotes (so that it is quoted and every quote doubled), and the
    // text of it is no longer than what is read.
    let quotes = "'".repeat(1024);
    let honest = format!("{}.'{}'.{}", "a".repeat(1024), quotes.replace('\'', "''"), "c".repeat(1024));
    assert!(honest.len() <= MAX_REFERENCE_BYTES, "{}", honest.len());
    let addr = cell_address(&honest).unwrap();
    assert_eq!(addr, named(&"a".repeat(1024), &quotes, &"c".repeat(1024)));
    assert_eq!(reference_name(&addr).as_deref(), Some(honest.as_str()));
}

/// The real time of hostile references: a figure to read, by hand (`cargo test -- --ignored --nocapture`), not a gate.
#[test]
#[ignore = "a time, to be read by hand"]
fn hostile_references_take_a_moment() {
    let started = Instant::now();
    let mut seen = 0;
    for n in 0..2_000usize {
        let text = format!("{}'{}", "s.".repeat(n % 3_000), "x'".repeat(n % 3_000));
        if cell_address(&text).is_ok() {
            seen += 1;
        }
    }
    eprintln!("2,000 hostile references in {:?} ({seen} read)", started.elapsed());
    assert!(started.elapsed().as_secs() < 30);
}

// --- evaluate_text ---

fn shape_sheet(owner: Owner) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(owner, 0))
}

/// One page with one shape, `ID` 1: `Width` 2 in, `LocPinX` `Width*0.5`, a `User.Total` worked out from them, and a `Scratch` row.
fn shape_doc() -> (Document, SheetRef) {
    let shape = ShapeBuilder::new(1)
        .cell_u("Width", "2", "IN")
        .cell_u("Height", "1", "IN")
        .formula("LocPinX", "Width*0.5", "1")
        .row_formula("User", None, RowKey::name("Total"), "Value", "Width+LocPinX", "3")
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "7");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    (doc, shape_sheet(Owner::Page(1)))
}

fn evaluate(doc: &Document, sheet: &SheetRef, at: Option<&CellAddr>, text: &str) -> Evaluated {
    evaluate_text(doc, sheet, at, text, &Budget::default())
}

fn value_of(evaluated: &Evaluated) -> &Value {
    match &evaluated.result {
        EvalResult::Value(v) => v,
        other => panic!("not a value: {other:?}"),
    }
}

#[test]
fn one_over_zero_reads_div_zero_and_the_document_is_the_same_after() {
    let (doc, sheet) = shape_doc();
    let before = doc.clone();
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Height")), "1/0");
    assert_eq!(e.result, EvalResult::Value(Value::Error(CellError::DivZero)));
    assert!(!e.budget_exhausted);
    assert!(e.steps > 0);
    assert_eq!(e.nodes, 3, "the parse, as the formula crate counts it");
    assert_eq!(doc, before, "nothing is written");
}

#[test]
fn a_text_that_ends_where_an_operand_is_due_gives_the_parse_error_at_the_end() {
    let (doc, sheet) = shape_doc();
    // The brief of expected `expected_operand` here. The formula parser says `unexpected_end` for a text that ends where an operand is due
    // (its `ParseErrorKind::UnexpectedEnd`) and keeps `expected_operand` for a token that is no operand (`1+*2`); the offset is the one the brief gave.
    let e = evaluate(&doc, &sheet, None, "1+");
    assert_eq!(e.result, EvalResult::ParseError(ParseError { kind: ParseErrorKind::UnexpectedEnd, offset: 2 }));
    assert_eq!(e.nodes, 0);
    assert!(!e.budget_exhausted);
    let e = evaluate(&doc, &sheet, None, "1+*2");
    assert_eq!(e.result, EvalResult::ParseError(ParseError { kind: ParseErrorKind::ExpectedOperand, offset: 2 }));
}

#[test]
fn a_parse_error_is_where_the_text_given_goes_wrong() {
    let (doc, sheet) = shape_doc();
    // The offset is into the text as given, white space and a leading `=` included, so that a window can mark the fault where it was typed.
    for (text, kind, offset) in [
        ("  1+", ParseErrorKind::UnexpectedEnd, 4),
        ("=1+*2", ParseErrorKind::ExpectedOperand, 3),
        ("(1+2", ParseErrorKind::ExpectedCloseParen, 4),
        ("\"abc", ParseErrorKind::UnterminatedString, 0),
        ("", ParseErrorKind::Empty, 0),
        ("   ", ParseErrorKind::Empty, 3),
    ] {
        let e = evaluate(&doc, &sheet, None, text);
        match e.result {
            EvalResult::ParseError(p) => assert_eq!((p.kind, p.offset), (kind, offset), "`{text}`"),
            other => panic!("`{text}`: {other:?}"),
        }
    }
}

#[test]
fn a_formula_is_worked_out_on_the_sheet_it_is_given_for() {
    let (doc, sheet) = shape_doc();
    let q = |v: f64, u: Unit| Value::Number(Quantity::new(v, u));
    for (text, expected) in [
        ("1+1", Value::num(2.0)),
        ("Width*2", q(4.0, Unit::Inch)),
        ("=Width*2", q(4.0, Unit::Inch)),
        ("LocPinX+Width", q(3.0, Unit::Inch)),
        ("User.Total*2", q(6.0, Unit::Inch)),
        ("Scratch.A1", Value::num(7.0)),
        ("Geometry1.X2", Value::Error(CellError::Ref)),
        ("\"a\"&\"b\"", Value::text("ab")),
        ("IF(Width>1,\"wide\",\"narrow\")", Value::text("wide")),
        ("Nothere", Value::num(0.0)),
    ] {
        let e = evaluate(&doc, &sheet, None, text);
        let got = value_of(&e);
        match (got, &expected) {
            (Value::Number(a), Value::Number(b)) => {
                assert!((a.stored - b.stored).abs() < 1e-9 && a.unit == b.unit, "`{text}`: {got:?} against {expected:?}")
            }
            _ => assert_eq!(got, &expected, "`{text}`"),
        }
    }
}

#[test]
fn a_bare_number_takes_the_unit_the_cell_holds() {
    // The cell supplies the unit it last held, else its default. Only the unit changes, never the stored number.
    let (doc, sheet) = shape_doc();
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Width")), "3");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(3.0, Unit::Inch)));
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("LocPinX")), "3");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(3.0, Unit::DrawingDefault)), "the document's default unit of a length");
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Height")), "1+1");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(2.0, Unit::Inch)), "a formula's bare result as well");
    // A cell of no unit takes the number as it is, and so does no cell at all.
    let e = evaluate(&doc, &sheet, Some(&ix("Scratch", None, 0, "A")), "3");
    assert_eq!(value_of(&e), &Value::num(3.0));
    let e = evaluate(&doc, &sheet, None, "3");
    assert_eq!(value_of(&e), &Value::num(3.0));
    // A number that carries a unit keeps it.
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Width")), "3 mm");
    assert_eq!(value_of(&e), &Value::Number(Quantity::from_value(3.0, Unit::Millimetre)));
}

#[test]
fn a_function_the_evaluator_lacks_is_unsupported_and_not_an_error() {
    let (doc, sheet) = shape_doc();
    let e = evaluate(&doc, &sheet, None, "NOSUCHFUNCTION(1)");
    assert_eq!(value_of(&e), &Value::unsupported("NOSUCHFUNCTION"));
    assert!(!e.budget_exhausted);
}

#[test]
fn an_action_cell_is_parsed_and_never_worked_out() {
    let shape = ShapeBuilder::new(1).cell_u("Width", "2", "IN").row_cell("Actions", None, RowKey::name("Row_1"), "Menu", "Go");
    let doc = DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build();
    let before = doc.clone();
    let sheet = shape_sheet(Owner::Page(1));
    let action = named("Actions", "Row_1", "Action");
    let e = evaluate(&doc, &sheet, Some(&action), "SETF(GetRef(Width),9)");
    assert_eq!(e.result, EvalResult::ActionCell);
    assert_eq!(e.nodes, diacraft_formula::parse("SETF(GetRef(Width),9)").node_count(), "it was parsed");
    assert!(e.nodes > 0);
    // A text that does not parse is refused there as anywhere.
    let e = evaluate(&doc, &sheet, Some(&action), "SETF(");
    assert!(matches!(e.result, EvalResult::ParseError(_)), "{e:?}");
    // The cells that run on an event are the same: no `SETF` runs.
    for cell in ["EventDblClick", "EventDrop", "EventMultiDrop", "EventXFMod", "TheData", "TheText"] {
        let e = evaluate(&doc, &sheet, Some(&CellAddr::direct(cell)), "SETF(GetRef(Width),9)");
        assert_eq!(e.result, EvalResult::ActionCell, "{cell}");
    }
    assert_eq!(doc, before);
}

#[test]
fn a_text_that_reads_the_cell_it_is_for_is_a_cycle() {
    // What a commit of it would give: a cell that reads itself is a loop (`rules::LOOP_ERROR`), not the old cell plus one.
    let (doc, sheet) = shape_doc();
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Width")), "Width+1");
    assert_eq!(value_of(&e), &Value::Error(CellError::Value));
    // Through a cell that reads it (`User.Total` reads `Width`) the cycle is calc's own to work out (a failing cell reads its last good value):
    // the run ends, within its budget, with a value.
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Width")), "User.Total");
    assert!(matches!(e.result, EvalResult::Value(_)) && !e.budget_exhausted, "{e:?}");
    // A text for another cell reads Width as the document holds it.
    let e = evaluate(&doc, &sheet, Some(&CellAddr::direct("Height")), "Width+1");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(3.0, Unit::Inch)));
}

#[test]
fn a_sheet_that_is_not_there_reads_no_reference() {
    let (doc, _) = shape_doc();
    let gone = shape_sheet(Owner::Page(9));
    let e = evaluate(&doc, &gone, Some(&CellAddr::direct("Width")), "Height");
    assert_eq!(value_of(&e), &Value::Error(CellError::Ref));
    let e = evaluate(&doc, &gone, Some(&CellAddr::direct("Width")), "1+1");
    assert_eq!(value_of(&e), &Value::num(2.0));
}

#[test]
fn a_master_a_style_a_page_and_the_document_have_sheets_to_try_a_formula_on() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell_u("Width", "4", "IN"));
    let doc = DocBuilder::new().master(1, master).page(1, PageBuilder::new().cell_u("PageWidth", "11", "IN")).doc_cell("DocCell", "8").build();
    let e = evaluate(&doc, &shape_sheet(Owner::Master(1)), None, "Width*2");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(8.0, Unit::Inch)));
    let e = evaluate(&doc, &SheetRef::Page(1), None, "PageWidth-1");
    assert_eq!(value_of(&e), &Value::Number(Quantity::new(10.0, Unit::Inch)));
    let e = evaluate(&doc, &SheetRef::Document, None, "DocCell*2");
    assert_eq!(value_of(&e), &Value::num(16.0));
    let e = evaluate(&doc, &SheetRef::Style(0), None, "EnableLineProps+0");
    assert_eq!(value_of(&e), &Value::num(1.0));
}

#[test]
fn a_spent_budget_is_said_and_the_value_is_the_loop_error() {
    let (doc, sheet) = shape_doc();
    let text = vec!["1"; 100].join("+");
    let e = evaluate_text(&doc, &sheet, None, &text, &Budget::with_steps(20));
    assert!(e.budget_exhausted, "{e:?}");
    assert_eq!(e.result, EvalResult::Value(Value::Error(CellError::Value)));
    assert!(e.steps <= 20, "the steps never pass the budget: {}", e.steps);
    // The same text with room works out.
    let e = evaluate_text(&doc, &sheet, None, &text, &Budget::with_steps(100_000));
    assert!(!e.budget_exhausted);
    assert_eq!(value_of(&e), &Value::num(100.0));
}

#[test]
fn the_parse_of_a_long_text_is_paid_for_before_it_is_made() {
    let (doc, sheet) = shape_doc();
    let text = "1+".repeat(1 << 19);
    // 1 MiB of text costs a step for each 64 bytes; with 100 steps it is not parsed at all.
    let e = evaluate_text(&doc, &sheet, None, &text, &Budget::with_steps(100));
    assert!(e.budget_exhausted);
    assert_eq!(e.nodes, 0, "not parsed");
    assert_eq!(e.result, EvalResult::Value(Value::Error(CellError::Value)));
    // With room, the parser's own limit speaks: the tree would be too deep.
    let e = evaluate_text(&doc, &sheet, None, &text, &Budget::with_steps(1_000_000));
    assert_eq!(e.result, EvalResult::ParseError(ParseError { kind: ParseErrorKind::TooDeep, offset: e_offset(&e) }));
    assert!(e.steps >= 16_384, "the text's length at 64 bytes a step: {}", e.steps);
    // Past the parser's own limit of a mebibyte.
    let over = "1".repeat((1 << 20) + 1);
    let e = evaluate_text(&doc, &sheet, None, &over, &Budget::with_steps(1_000_000));
    assert!(matches!(e.result, EvalResult::ParseError(ParseError { kind: ParseErrorKind::TooLong, .. })), "{e:?}");
}

fn e_offset(e: &Evaluated) -> usize {
    match &e.result {
        EvalResult::ParseError(p) => p.offset,
        _ => 0,
    }
}

/// Runs `f` on a thread with a stack of 512 KiB, half what the browser gives, so that a recursion nothing bounds overflows it here.
fn small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().unwrap()
}

/// A page of one shape whose `Scratch.A1` is 1 and each row after reads the one before and adds 1: `n` cells deep, the last worth `n`.
fn chain_doc(n: u32) -> Document {
    let mut shape = ShapeBuilder::new(1).row_cell("Scratch", None, RowKey::Ix(0), "A", "1");
    for k in 1..n {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Scratch.A{k}+1"), "0");
    }
    DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build()
}

#[test]
fn a_formula_that_reads_the_end_of_a_long_chain_of_cells_does_not_recurse() {
    // 5,000 cells deep, read by the text from the last, on a stack of half what the browser gives.
    let n = 5_000u32;
    let doc = chain_doc(n);
    let before = doc.clone();
    let (e, doc) = small_stack(move || {
        let e = evaluate_text(&doc, &shape_sheet(Owner::Page(1)), None, &format!("Scratch.A{n}*2"), &Budget::default());
        (e, doc)
    });
    assert_eq!(value_of(&e), &Value::num(f64::from(n) * 2.0), "{e:?}");
    assert!(e.steps < 200_000, "a step or so a cell: {}", e.steps);
    assert_eq!(doc, before);
}

#[test]
fn a_text_that_names_a_cell_many_times_has_it_worked_out_once() {
    // 30,000 references to the end of a chain of 2,000 (about as many as the parser's limit of nodes allows): the chain is worked out once and the
    // references each cost a few steps, not a walk.
    let (n, names) = (2_000u32, 30_000usize);
    let doc = chain_doc(n);
    let text = format!("SUM({})", vec![format!("Scratch.A{n}"); names].join(","));
    let e = evaluate_text(&doc, &shape_sheet(Owner::Page(1)), None, &text, &Budget::default());
    assert_eq!(value_of(&e), &Value::num(f64::from(n) * names as f64), "{:?}", e.result);
    let (names, n) = (names as u64, u64::from(n));
    assert!(e.steps <= 8 * names + 20 * n, "{} steps for {names} names and {n} cells", e.steps);
}

#[test]
fn a_text_that_names_the_cell_it_is_for_in_every_argument_is_a_loop_within_the_budget() {
    // The cell is held as being worked out, so each of the 20,000 names finds a loop at once and none walks what the cell reads.
    let doc = chain_doc(2_000);
    let sheet = shape_sheet(Owner::Page(1));
    let at = ix("Scratch", None, 1_999, "A");
    let text = format!("SUM({})", vec!["Scratch.A2000"; 20_000].join(","));
    let e = evaluate_text(&doc, &sheet, Some(&at), &text, &Budget::default());
    assert_eq!(value_of(&e), &Value::Error(CellError::Value), "{:?}", e.result);
    assert!(e.steps <= 400_000, "{} steps", e.steps);
}

#[test]
fn hostile_formulas_end_within_the_budget() {
    let (doc, sheet) = shape_doc();
    let budget = Budget::with_steps(50_000);
    let nest = |open: &str, close: &str, inner: &str, depth: usize| format!("{}{}{}", open.repeat(depth), inner, close.repeat(depth));
    let texts: Vec<(&str, String)> = vec![
        ("brackets to the parser's limit", nest("(", ")", "1", 99)),
        ("brackets past it", nest("(", ")", "1", 400)),
        ("IF to the limit", nest("IF(1,", ",0)", "1", 50)),
        ("signs", format!("{}1", "-".repeat(90))),
        ("a long sum", vec!["Width"; 500].join("+")),
        ("percent signs", format!("1{}", "%".repeat(100_000))),
        ("a string of a megabyte", format!("\"{}\"", "a".repeat(1 << 20))),
        ("a power tower", vec!["9"; 90].join("^")),
        ("references to a missing row", vec!["Prop.Nothere"; 2_000].join("+")),
        ("a text read as a formula", "VALUE(\"1+VALUE(\\\"1+1\\\")\")".to_owned()),
        ("a string made a formula over and over", format!("{}\"1\"{}", "EVALTEXT(".repeat(60), ")".repeat(60))),
    ];
    for (why, text) in &texts {
        let e = evaluate_text(&doc, &sheet, Some(&CellAddr::direct("Height")), text, &budget);
        assert!(e.steps <= budget.steps, "{why}: {} steps", e.steps);
        match &e.result {
            EvalResult::Value(_) | EvalResult::ParseError(_) => {}
            EvalResult::ActionCell => panic!("{why}: Height is no action cell"),
        }
    }
}

#[test]
fn an_evaluation_with_the_options_of_a_session_reads_its_clock() {
    let (doc, sheet) = shape_doc();
    let clock = crate::fixed_clock("2026-11-25T10:20:30", 0);
    let options = EvalOptions { clock, ..EvalOptions::default() };
    let none = diacraft_calc::evaluate_text_with(&doc, &sheet, None, "YEAR(NOW())", &EvalOptions::default(), &Budget::default());
    assert_eq!(value_of(&none), &Value::unsupported("NOW"), "no clock, no NOW");
    let some = diacraft_calc::evaluate_text_with(&doc, &sheet, None, "YEAR(NOW())", &options, &Budget::default());
    assert_eq!(value_of(&some), &Value::num(2026.0));
}

/// The real time of crafted formulas at the budget a window gives a dry run (200,000 steps): figures to read by hand, not a gate.
#[test]
#[ignore = "a time, to be read by hand"]
fn hostile_formulas_take_a_moment() {
    let doc = chain_doc(5_000);
    let sheet = shape_sheet(Owner::Page(1));
    let budget = Budget::with_steps(200_000);
    let at = ix("Scratch", None, 4_999, "A");
    let texts: Vec<(&str, String, Option<CellAddr>)> = vec![
        ("a mebibyte of 1+", "1+".repeat(1 << 19), None),
        ("a mebibyte of one number", "1".repeat(1 << 20), None),
        ("30,000 names of one cell", format!("SUM({})", vec!["Scratch.A5000"; 30_000].join(",")), None),
        ("the end of a chain of 5,000", "Scratch.A5000".to_owned(), None),
        ("the cell it is for, 20,000 times", format!("SUM({})", vec!["Scratch.A5000"; 20_000].join(",")), Some(at)),
        ("a long sum of names", vec!["Scratch.A5000"; 500].join("+"), None),
        ("a string of a mebibyte joined to itself", format!("\"{}\"&\"{}\"", "a".repeat(1 << 19), "b".repeat(1 << 19)), None),
        ("EVALTEXT nested", format!("{}\"1\"{}", "EVALTEXT(".repeat(60), ")".repeat(60)), None),
    ];
    for (why, text, at) in &texts {
        let started = Instant::now();
        let e = evaluate_text(&doc, &sheet, at.as_ref(), text, &budget);
        let took = started.elapsed();
        eprintln!("{why}: {took:?}, {} steps, exhausted {}", e.steps, e.budget_exhausted);
        assert!(took.as_secs() < 20, "{why}: {took:?}");
    }
}
