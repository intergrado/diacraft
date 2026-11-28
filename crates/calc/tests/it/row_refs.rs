//! The references to a shape's connection rows that a formula holds (`diacraft_calc::row_refs`), found and renumbered character for character.
//!
//! The vectors are the design's (the plan's task): a reference is `Connections.X1`, `Sheet.5!Connections.DirY12`, `Connections.Row_3.X` (a row's name in
//! any case) or a bare `Connections.X` (row 1); text in a string or a quoted name is no reference; a reference after another qualifier
//! is left; a number past 4,294,967,295 is an error in words and never a wrap; and every loop is charged to a budget by the bytes it passes over.

use diacraft_calc::row_refs::{CONNECTIONS, RowName, RowRef, RowRefError, RowTarget, find, mentions, renumber};
use proptest::prelude::*;

fn found(text: &str) -> Vec<RowRef> {
    let mut budget = 10_000_000;
    find(text, CONNECTIONS, &mut budget).unwrap()
}

fn tail<'a>(text: &'a str, r: &RowRef) -> &'a str {
    text.get(r.start..r.end).unwrap()
}

/// `text` renumbered for shape 5 (its own formula) by `map`, with a budget that is not the point of the test.
fn renumbered(text: &str, of: RowTarget, map: &dyn Fn(&RowName) -> Option<RowName>) -> Option<String> {
    let mut budget = 10_000_000;
    renumber(text, CONNECTIONS, of, map, &mut budget).unwrap()
}

/// Row numbers one higher: `X1` is `X2`, and a name is kept.
fn up(row: &RowName) -> Option<RowName> {
    match row {
        RowName::Number(n) => Some(RowName::Number(n + 1)),
        RowName::Default => Some(RowName::Number(2)),
        RowName::Name(_) => None,
    }
}

/// The inverse of [`up`] for the numbers it made.
fn down(row: &RowName) -> Option<RowName> {
    match row {
        RowName::Number(n) if *n > 1 => Some(RowName::Number(n - 1)),
        _ => None,
    }
}

#[test]
fn the_forms_of_a_reference_to_a_row_are_found_with_their_parts() {
    let text = "Connections.X1";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((refs[0].sheet, &refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (None, &RowName::Number(1), "X", "X1"));

    let text = "Sheet.5!Connections.DirY12";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((refs[0].sheet, &refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (Some(5), &RowName::Number(12), "DirY", "DirY12"));

    let text = "Connections.Row_3.X";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((refs[0].sheet, &refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (None, &RowName::Name("Row_3".into()), "X", "Row_3.X"));

    // A row's name in another case, with the section in another case too: the section is found whatever its case (a reader that binds only the exact
    // spelling leaves the reference alone, and it is then kept right for a reader that does not).
    let text = "connections.row_3.X";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((&refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (&RowName::Name("row_3".into()), "X", "row_3.X"));

    // No row number is row 1.
    let text = "Connections.X";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((refs[0].sheet, &refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (None, &RowName::Default, "X", "X"));

    // The row number after the cell in brackets, the form a cell of more than one letter has in the spec's table (`Char.Size[2]`).
    let text = "Connections.DirX[7]";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((&refs[0].row, refs[0].cell.as_str(), tail(text, &refs[0])), (&RowName::Number(7), "DirX", "DirX[7]"));

    // A glue formula holds two.
    let text = "PAR(PNT(Sheet.4!Connections.X3,Sheet.4!Connections.Y3))";
    let refs = found(text);
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert_eq!((refs[0].sheet, refs[0].cell.as_str(), &refs[0].row), (Some(4), "X", &RowName::Number(3)));
    assert_eq!((refs[1].sheet, refs[1].cell.as_str(), &refs[1].row), (Some(4), "Y", &RowName::Number(3)));
    assert!(refs[0].end <= refs[1].start, "in order, and apart: {refs:?}");
}

#[test]
fn every_cell_of_a_row_is_a_cell_of_a_reference_and_the_others_are_not() {
    for cell in ["X", "Y", "DirX", "DirY", "Type", "A", "B", "C", "D", "AutoGen", "Prompt"] {
        let text = format!("Connections.{cell}4");
        let refs = found(&text);
        assert_eq!(refs.len(), 1, "{text}");
        assert_eq!((refs[0].cell.as_str(), &refs[0].row), (cell, &RowName::Number(4)));
    }
    // A cell's name matches exactly; a name no cell of a row has is no reference to a row.
    for text in [
        "Connections.x1",
        "Connections.DIRX1",
        "Connections.Foo1",
        "Connections.Row_3",
        "Connections.X1a",
        "Connections.1",
        "Connections.X1.Y.Z",
        "Connections",
    ] {
        assert_eq!(found(text), Vec::new(), "{text}");
    }
    // A name that looks like a cell and a number is a row's name when a cell follows it: row `X1`, cell `Y`.
    let refs = found("Connections.X1.Y");
    assert_eq!(refs.len(), 1);
    assert_eq!((&refs[0].row, refs[0].cell.as_str()), (&RowName::Name("X1".into()), "Y"));
}

#[test]
fn text_in_a_string_or_a_quoted_name_is_no_reference() {
    for text in ["\"Connections.X1\"", "'Connections'!X1", "'Connections.X1'!Width", "\"say Sheet.5!Connections.X1\"", "\"a\"&\"Connections.X1\""] {
        assert_eq!(found(text), Vec::new(), "{text}");
    }
    // A doubled quote inside a string is a quote and not the end of it; after the string the formula goes on.
    let text = "IF(A,\"say \"\"Connections.X1\"\"\",Connections.X2)";
    let refs = found(text);
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!((&refs[0].row, tail(text, &refs[0])), (&RowName::Number(2), "X2"));
}

#[test]
fn another_section_another_qualifier_and_a_word_that_only_ends_in_the_name_are_left() {
    for text in [
        "Geometry1.X1",
        "Controls.X1",
        "Scratch.X1",
        "MyConnections.X1",
        "Connections2.X1",
        "Pages[P]!Sheet.5!Connections.X1",
        "Pages[P]!Connections.X1",
        "ThePage!Connections.X1",
        "TheDoc!Connections.X1",
        "Door!Connections.X1",
        "'Left-Door'!Connections.X1",
        "LISTSHEETREF()!Connections.X1",
        "MySheet.5!Connections.X1",
        "Sheet.5.6!Connections.X1",
        "Sheet.99999999999!Connections.X1",
        "Sheet.!Connections.X1",
        "1.5Connections.X1",
    ] {
        assert_eq!(found(text), Vec::new(), "{text}");
    }
    // Another shape's reference is found, with its sheet, for the caller to leave or not.
    let refs = found("Sheet.6!Connections.X1");
    assert_eq!((refs.len(), refs[0].sheet), (1, Some(6)));
    // The qualifier is `Sheet` in any case, with a number as long as the id; white space may stand round the `!` as the parser allows.
    for text in ["sheet.05!Connections.X1", "SHEET.5 ! Connections.X1", "Sheet.5!  Connections.X1"] {
        let refs = found(text);
        assert_eq!(refs.len(), 1, "{text}");
        assert_eq!(refs[0].sheet, Some(5), "{text}");
    }
}

#[test]
fn only_the_rows_of_the_shape_asked_for_are_renumbered() {
    let text = "Connections.X1+Sheet.5!Connections.X1+Sheet.6!Connections.X1";
    // A formula of shape 5's own sheet: its own references and those that name it.
    assert_eq!(renumbered(text, RowTarget::own(5), &up).unwrap(), "Connections.X2+Sheet.5!Connections.X2+Sheet.6!Connections.X1");
    // A formula of another shape: only the references that name shape 5.
    assert_eq!(renumbered(text, RowTarget::of_sheet(5), &up).unwrap(), "Connections.X1+Sheet.5!Connections.X2+Sheet.6!Connections.X1");
    assert_eq!(renumbered("Sheet.6!Connections.X1", RowTarget::own(5), &up), None);
    assert_eq!(renumbered("Pages[P]!Sheet.5!Connections.X1", RowTarget::own(5), &up), None);
}

#[test]
fn an_identity_map_gives_none_and_a_map_with_its_inverse_gives_the_text_back_byte_for_byte() {
    let text = "PAR(PNT(Sheet.5!Connections.X3,Sheet.5!Connections.Y3))+Connections.DirX[7]+1*Connections.Row_2.X+\"Connections.X9\"+Pages[P]!Sheet.5!Connections.X9";
    assert_eq!(renumbered(text, RowTarget::own(5), &|_| None), None);
    let same = |row: &RowName| Some(row.clone());
    assert_eq!(renumbered(text, RowTarget::own(5), &same), None, "a map that names each row as it is changes no byte");
    let moved = renumbered(text, RowTarget::own(5), &up).unwrap();
    assert_eq!(
        moved,
        "PAR(PNT(Sheet.5!Connections.X4,Sheet.5!Connections.Y4))+Connections.DirX[8]+1*Connections.Row_2.X+\"Connections.X9\"+Pages[P]!Sheet.5!Connections.X9"
    );
    assert_eq!(renumbered(&moved, RowTarget::own(5), &down).unwrap(), text);
}

#[test]
fn a_bare_reference_is_row_one_and_a_row_that_moves_gets_its_number() {
    assert_eq!(
        renumbered("Sheet.5!Connections.X", RowTarget::of_sheet(5), &|row| (row == &RowName::Number(1)).then_some(RowName::Number(3))).unwrap(),
        "Sheet.5!Connections.X3"
    );
    // Row 1 staying row 1 is no change, whichever way the map says it.
    assert_eq!(renumbered("Connections.X", RowTarget::own(5), &|row| Some(row.clone())), None);
    assert_eq!(renumbered("Connections.X1", RowTarget::own(5), &|_| Some(RowName::Number(1))), None);
    // Leading zeros are kept when the row is not moved, as every other byte is.
    assert_eq!(
        renumbered("Connections.X05+Connections.Y06", RowTarget::own(5), &|row| (row == &RowName::Number(6)).then_some(RowName::Number(7))).unwrap(),
        "Connections.X05+Connections.Y7"
    );
}

#[test]
fn a_named_row_is_renamed_in_its_own_form_and_a_row_can_change_form() {
    let to_name =
        |to: &'static str| move |row: &RowName| matches!(row, RowName::Name(n) if n.eq_ignore_ascii_case("row_3")).then(|| RowName::Name(to.into()));
    assert_eq!(renumbered("Sheet.5!Connections.Row_3.X", RowTarget::of_sheet(5), &to_name("Row_7")).unwrap(), "Sheet.5!Connections.Row_7.X");
    // A name that is no plain word is written in quotes, a quote in it doubled; and read the same way.
    assert_eq!(renumbered("Connections.Row_3.Y", RowTarget::own(5), &to_name("Row 9")).unwrap(), "Connections.'Row 9'.Y");
    assert_eq!(renumbered("Connections.Row_3.Y", RowTarget::own(5), &to_name("It's")).unwrap(), "Connections.'It''s'.Y");
    let refs = found("Connections.'It''s'.Y");
    assert_eq!(refs.len(), 1);
    assert_eq!((&refs[0].row, refs[0].cell.as_str(), tail("Connections.'It''s'.Y", &refs[0])), (&RowName::Name("It's".into()), "Y", "'It''s'.Y"));
    // A name to a number and a number to a name: the cell is kept.
    assert_eq!(renumbered("Connections.Row_3.DirX", RowTarget::own(5), &|_| Some(RowName::Number(4))).unwrap(), "Connections.DirX4");
    assert_eq!(renumbered("Connections.DirX4", RowTarget::own(5), &|_| Some(RowName::Name("Row_9".into()))).unwrap(), "Connections.Row_9.DirX");
    assert_eq!(
        renumbered("Connections.DirX[4]", RowTarget::own(5), &|_| Some(RowName::Number(2))).unwrap(),
        "Connections.DirX[2]",
        "a row number in brackets stays in brackets"
    );
}

#[test]
fn a_map_that_names_no_row_is_refused_in_words() {
    let mut budget = 1_000_000;
    let e = renumber("Connections.X1", CONNECTIONS, RowTarget::own(5), &|_| Some(RowName::Number(0)), &mut budget).unwrap_err();
    assert_eq!(e, RowRefError::Unwritable("row numbers start at 1".into()));
    assert_eq!(e.to_string(), "a reference cannot be written: row numbers start at 1");
    for bad in [RowName::Name(String::new()), RowName::Name("x".repeat(1_025))] {
        let e = renumber("Connections.X1", CONNECTIONS, RowTarget::own(5), &|_| Some(bad.clone()), &mut budget).unwrap_err();
        assert!(matches!(e, RowRefError::Unwritable(_)), "{e:?}");
    }
}

#[test]
fn a_row_number_past_the_largest_is_an_error_in_words_and_never_a_wrap() {
    let mut budget = 1_000_000;
    let text = "1+Connections.X4294967296";
    let e = find(text, CONNECTIONS, &mut budget).unwrap_err();
    assert_eq!(e, RowRefError::NumberTooLarge { at: 15 });
    assert_eq!(e.to_string(), "a row number in a reference is larger than 4294967295 (at byte 15)");
    assert_eq!(renumber(text, CONNECTIONS, RowTarget::own(5), &up, &mut budget).unwrap_err(), RowRefError::NumberTooLarge { at: 15 });
    // Another shape's reference with a number that large is not this renumbering's concern.
    assert_eq!(renumber("Sheet.6!Connections.X4294967296", CONNECTIONS, RowTarget::own(5), &up, &mut budget), Ok(None));
    // The largest number is read, and cannot be raised without a wrap: the map's own to give.
    let refs = found("Connections.X4294967295");
    assert_eq!(refs[0].row, RowName::Number(u32::MAX));
    assert_eq!(renumbered("Connections.X4294967295", RowTarget::own(5), &|_| Some(RowName::Number(7))).unwrap(), "Connections.X7");
}

#[test]
fn a_section_the_scanner_does_not_know_is_refused_and_one_it_knows_is_found_in_any_case() {
    let mut budget = 1_000;
    assert_eq!(find("Scratch.A1", "Scratch", &mut budget), Err(RowRefError::UnknownSection("Scratch".into())));
    assert_eq!(budget, 1_000, "a refusal costs nothing");
    for section in ["connections", "CONNECTIONS", "Connections"] {
        assert_eq!(find("Connections.X1", section, &mut budget).unwrap().len(), 1, "{section}");
    }
}

#[test]
fn the_scan_is_charged_by_the_bytes_it_passes_over() {
    // A text with no reference costs its length, one for each byte.
    for text in ["", "1+2", "Width*2+\"Connections.X1\"", "héllo&Sheet.5!Width"] {
        let mut budget = 100;
        find(text, CONNECTIONS, &mut budget).unwrap();
        assert_eq!(100 - budget, text.len(), "{text}");
    }
    // A reference costs the same: its bytes are passed over once.
    let mut budget = 100;
    let refs = find("Sheet.5!Connections.X12", CONNECTIONS, &mut budget).unwrap();
    assert_eq!((refs.len(), 100 - budget), (1, "Sheet.5!Connections.X12".len()));
    // A renumbering adds the bytes it writes.
    let mut budget = 100;
    renumber("Sheet.5!Connections.X12", CONNECTIONS, RowTarget::own(5), &|_| Some(RowName::Number(7)), &mut budget).unwrap();
    assert_eq!(100 - budget, "Sheet.5!Connections.X12".len() + "X7".len());
    // A scan that is not given its bytes stops, in words, and spends what it had.
    let mut budget = 10;
    let e = find("Sheet.5!Connections.X12", CONNECTIONS, &mut budget).unwrap_err();
    assert_eq!(e, RowRefError::BudgetSpent);
    assert_eq!(e.to_string(), "the formula is too long to look through for references to the rows of a section (the work budget is spent)");
    assert_eq!(budget, 0);
}

/// A formula of 65,536 bytes made of the one reference.
fn a_formula_of_tokens() -> String {
    let token = "Connections.X1+";
    let mut text = token.repeat(65_536 / token.len() + 1);
    text.truncate(65_536);
    text
}

#[test]
fn a_formula_of_65536_bytes_of_references_is_renumbered_within_a_counted_budget_and_refused_past_it() {
    let text = a_formula_of_tokens();
    assert_eq!(text.len(), 65_536);
    let refs_in_text = text.matches("Connections.X1").count();
    assert!(refs_in_text > 4_000, "{refs_in_text}");
    // Within the budget: the bytes, and the two bytes written for each reference.
    let mut budget = 4 * text.len();
    let out = renumber(&text, CONNECTIONS, RowTarget::own(5), &up, &mut budget).unwrap().unwrap();
    assert_eq!(out.matches("Connections.X2").count(), refs_in_text);
    assert_eq!(out.len(), text.len(), "one digit for one digit");
    assert_eq!(4 * text.len() - budget, text.len() + 2 * refs_in_text, "counted: each byte once, each byte written once");
    // Past it: refused in words, and the text is not half done: no text comes out.
    let mut budget = text.len() - 1;
    assert_eq!(renumber(&text, CONNECTIONS, RowTarget::own(5), &up, &mut budget), Err(RowRefError::BudgetSpent));
    let mut budget = text.len() + refs_in_text;
    assert_eq!(renumber(&text, CONNECTIONS, RowTarget::own(5), &up, &mut budget), Err(RowRefError::BudgetSpent), "the bytes written count too");
}

#[test]
fn hostile_texts_are_refused_or_read_within_their_length_and_never_panic() {
    let mut hostile: Vec<String> = vec![
        "'".repeat(8_192),
        "\"".repeat(8_193),
        "!".repeat(8_192),
        "[".repeat(8_192),
        "Sheet.".repeat(2_000),
        "Sheet.5!".repeat(1_500),
        "Connections.".repeat(1_000),
        "Connections.X[".repeat(600),
        format!("Connections.X[{}]", " ".repeat(8_000)),
        format!("Connections.{}.X", "a".repeat(8_000)),
        format!("Connections.'{}'.X", "a".repeat(8_000)),
        format!("Sheet.5{}!Connections.X1", " ".repeat(8_000)),
        "(".repeat(8_192),
        "a.".repeat(4_000),
        "é".repeat(4_000),
        "Pages[".repeat(1_000),
        "Sheet.5!Connections.X1!".repeat(300),
        "#REF!".repeat(2_000),
    ];
    hostile.push(format!("{}Connections.X1", "x".repeat(8_000)));
    for text in &hostile {
        let mut budget = 3 * text.len() + 64;
        let before = budget;
        match find(text, CONNECTIONS, &mut budget) {
            Ok(refs) => {
                for r in &refs {
                    assert!(text.get(r.start..r.end).is_some() && r.start <= r.end, "a span on character boundaries");
                }
                assert!(before - budget <= 2 * text.len(), "scanned {} bytes of {}", before - budget, text.len());
            }
            Err(e) => assert_eq!(e, RowRefError::BudgetSpent),
        }
        let mut budget = 3 * text.len() + 64;
        assert_eq!(renumber(text, CONNECTIONS, RowTarget::own(5), &|_| None, &mut budget), Ok(None));
    }
}

#[test]
fn a_text_that_does_not_mention_the_section_is_known_without_a_scan() {
    assert!(mentions("Sheet.5!Connections.X1", CONNECTIONS));
    assert!(mentions("CONNECTIONS.x1", CONNECTIONS));
    assert!(mentions("héllo connections", CONNECTIONS));
    assert!(!mentions("Sheet.5!Width", CONNECTIONS));
    assert!(!mentions("Connection.X1", CONNECTIONS));
    assert!(!mentions("", CONNECTIONS));
}

#[test]
fn text_with_characters_of_more_than_one_byte_is_kept_character_for_character() {
    let text = "héllo&Sheet.5!Connections.X1&\"é\"&'ü'!Width&Connections.Row_é.X";
    assert_eq!(renumbered(text, RowTarget::own(5), &up).unwrap(), "héllo&Sheet.5!Connections.X2&\"é\"&'ü'!Width&Connections.Row_é.X");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// Any text of the tokens a formula about connection rows holds: no panic, spans on character boundaries and in order, and the map that changes nothing
    /// gives the text back (`None`).
    #[test]
    fn any_text_is_read_without_a_panic_and_an_identity_map_changes_no_byte(parts in proptest::collection::vec(proptest::sample::select(vec![
        "Connections", ".", "X", "Y", "DirX", "1", "12", "0", "4294967296", "Row_3", "row_3", "[", "]", "Sheet", "sheet", "!", " ", "'", "\"", "(", ")", ",", "+", "é", "Pages", "5",
    ]), 0..40)) {
        let text: String = parts.concat();
        let mut budget = 10_000_000;
        match find(&text, CONNECTIONS, &mut budget) {
            Ok(refs) => {
                // Every byte of the text is passed over once and charged one step.
                prop_assert_eq!(10_000_000 - budget, text.len());
                let mut at = 0;
                for r in &refs {
                    prop_assert!(text.get(r.start..r.end).is_some());
                    prop_assert!(r.start >= at && r.start <= r.end);
                    at = r.end;
                }
            }
            Err(e) => prop_assert!(matches!(e, RowRefError::NumberTooLarge { at: _ }), "an unexpected error: {e:?}"),
        }
        // A budget one step short of the text is refused in words, and nothing is left of it.
        if !text.is_empty() {
            let mut short = text.len() - 1;
            prop_assert_eq!(find(&text, CONNECTIONS, &mut short), Err(RowRefError::BudgetSpent));
            prop_assert_eq!(short, 0);
        }
        let mut budget = 10_000_000;
        let same = |row: &RowName| Some(match row { RowName::Default => RowName::Number(1), other => other.clone() });
        match renumber(&text, CONNECTIONS, RowTarget::own(5), &same, &mut budget) {
            Ok(out) => prop_assert_eq!(out, None),
            Err(e) => prop_assert!(matches!(e, RowRefError::NumberTooLarge { at: _ }), "an unexpected error: {e:?}"),
        }
    }

    /// A renumbering neither makes nor loses a reference: the text that comes out has as many, each of shape 5 one row higher and every other the row it had, and
    /// the bytes of a reference that was not moved are the bytes it had.
    #[test]
    fn a_renumbering_keeps_every_reference_and_moves_only_the_ones_asked_for(parts in proptest::collection::vec(proptest::sample::select(vec![
        "Connections.X1", "Connections.Y2", "Sheet.5!", "Sheet.6!", "Connections.DirX[3]", "Connections.Row_3.X", "Connections.X", "+", " ", "\"", "'", "Pages[P]!", "(", ")", "é",
    ]), 0..24)) {
        let text: String = parts.concat();
        let of = RowTarget::own(5);
        let mut budget = 10_000_000;
        let Ok(old) = find(&text, CONNECTIONS, &mut budget) else { return Ok(()) };
        let mut budget = 10_000_000;
        let Ok(Some(out)) = renumber(&text, CONNECTIONS, of, &up, &mut budget) else { return Ok(()) };
        let mut budget = 10_000_000;
        let new = find(&out, CONNECTIONS, &mut budget).unwrap();
        prop_assert_eq!(old.len(), new.len(), "{:?} gave {:?}", text, out);
        for (o, n) in old.iter().zip(&new) {
            prop_assert_eq!((o.sheet, &o.cell), (n.sheet, &n.cell));
            let applies = match o.sheet { None => of.unqualified, Some(id) => id == of.sheet };
            if applies {
                match (&o.row, &n.row) {
                    (RowName::Number(a), RowName::Number(b)) => prop_assert_eq!(a + 1, *b),
                    (RowName::Default, RowName::Number(b)) => prop_assert_eq!(*b, 2),
                    (a, b) => prop_assert_eq!(a, b),
                }
            } else {
                prop_assert_eq!(&o.row, &n.row);
                prop_assert_eq!(text.get(o.start..o.end), out.get(n.start..n.end));
            }
        }
    }
}
