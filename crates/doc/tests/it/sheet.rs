//! A sheet: cells directly under it, sections of rows of cells, and how a cell is addressed in one.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, DocError, Keying, Lookup, MAX_CELLS_PER_CONTAINER, MAX_ROWS_PER_SECTION, MAX_SECTIONS_PER_SHEET, Row, RowKey, Section,
    SectionKey, SectionName, Sheet, Value, keying,
};

fn num(v: &str) -> Cell {
    Cell::constant(Value::from_text(v))
}

fn value_of<'a>(sheet: &'a Sheet, addr: &CellAddr) -> Option<&'a str> {
    match sheet.lookup(addr) {
        Lookup::Found(c) => c.value.as_ref().map(Value::text),
        _ => None,
    }
}

#[test]
fn a_cell_directly_under_a_sheet_is_found_by_exact_name() {
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::direct("PinX"), num("3")).unwrap();
    assert_eq!(value_of(&sheet, &CellAddr::direct("PinX")), Some("3"));
    assert!(matches!(sheet.lookup(&CellAddr::direct("pinx")), Lookup::Missing), "a case variant is another cell");
    assert!(matches!(sheet.lookup(&CellAddr::direct("PinY")), Lookup::Missing));
    assert_eq!(sheet.cell("PinX").and_then(|c| c.value.as_ref()).map(Value::text), Some("3"));
    // Both spellings can live side by side, each verbatim.
    sheet.set_cell(&CellAddr::direct("pinx"), num("9")).unwrap();
    assert_eq!(value_of(&sheet, &CellAddr::direct("PinX")), Some("3"));
    assert_eq!(value_of(&sheet, &CellAddr::direct("pinx")), Some("9"));
}

#[test]
fn writing_a_cell_in_a_row_adds_the_section_and_row_as_files_do() {
    // The cell goes in a local row with the same key; the section and row are added if there are none.
    let mut sheet = Sheet::default();
    let addr = CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "X");
    assert!(matches!(sheet.lookup(&addr), Lookup::Missing));
    sheet.set_cell(&addr, num("1")).unwrap();
    assert_eq!(value_of(&sheet, &addr), Some("1"));
    assert_eq!(sheet.sections.len(), 1);
    let section = &sheet.sections[0];
    assert_eq!((section.name.as_str(), section.ix, section.del), ("Geometry", Some(0), false));
    assert_eq!((section.rows.len(), section.rows[0].ix, section.rows[0].name.as_deref()), (1, Some(2), None));
    // A second cell lands in the same row; a named row in a named section gets its name.
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(2), "Y"), num("2")).unwrap();
    assert_eq!(sheet.sections[0].rows.len(), 1);
    sheet.set_cell(&CellAddr::in_row("User", None, RowKey::name("Mine"), "Value"), num("7")).unwrap();
    let user = sheet.sections.iter().find(|s| s.name == "User").unwrap();
    assert_eq!((user.rows[0].name.as_deref(), user.rows[0].ix), (Some("Mine"), None));
}

#[test]
fn geometry_sections_are_told_apart_by_ix_and_other_sections_by_name() {
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), num("10")).unwrap();
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(1), RowKey::Ix(1), "X"), num("11")).unwrap();
    assert_eq!(sheet.sections.len(), 2);
    assert_eq!(value_of(&sheet, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")), Some("10"));
    assert_eq!(value_of(&sheet, &CellAddr::in_row("Geometry", Some(1), RowKey::Ix(1), "X")), Some("11"));
    assert!(matches!(sheet.lookup(&CellAddr::in_row("Geometry", Some(2), RowKey::Ix(1), "X")), Lookup::Missing));
}

#[test]
fn cells_directly_under_a_section_are_found_too() {
    // `NoFill`, `NoLine` and the rest sit on the Geometry section itself.
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::in_section("Geometry", Some(0), "NoFill"), num("1")).unwrap();
    assert_eq!(value_of(&sheet, &CellAddr::in_section("Geometry", Some(0), "NoFill")), Some("1"));
    assert!(matches!(sheet.lookup(&CellAddr::in_section("Geometry", Some(0), "NoLine")), Lookup::Missing));
}

#[test]
fn a_deleted_row_or_section_is_deleted_not_missing() {
    // `Del` on a row or section stops its values being inherited, so the cell is neither
    // found nor missing: it is gone.
    let mut sheet = Sheet::default();
    let gone = Row { ix: Some(3), del: true, ..Row::default() };
    let mut section = Section::new("Geometry", Some(0));
    section.rows.push(Arc::new(gone));
    sheet.sections.push(Arc::new(section));
    assert!(matches!(sheet.lookup(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(3), "X")), Lookup::Deleted));
    assert!(matches!(sheet.lookup(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(4), "X")), Lookup::Missing));

    let mut dead = Section::new("Scratch", None);
    dead.del = true;
    sheet.sections.push(Arc::new(dead));
    assert!(matches!(sheet.lookup(&CellAddr::in_row("Scratch", None, RowKey::Ix(0), "X")), Lookup::Deleted));
    assert!(matches!(sheet.lookup(&CellAddr::in_section("Scratch", None, "A")), Lookup::Deleted));
}

#[test]
fn rows_are_keyed_by_name_or_index_as_the_section_says() {
    assert_eq!(keying("User"), Keying::Named);
    assert_eq!(keying("Property"), Keying::Named);
    assert_eq!(keying("Actions"), Keying::Named);
    assert_eq!(keying("Control"), Keying::Named);
    assert_eq!(keying("Hyperlink"), Keying::Named);
    assert_eq!(keying("ActionTag"), Keying::Named);
    for s in ["Character", "Field", "FillGradient", "Geometry", "Layer", "LineGradient", "Paragraph", "Reviewer", "Scratch", "Tabs"] {
        assert_eq!(keying(s), Keying::Indexed, "{s}");
    }
    assert_eq!(keying("Connection"), Keying::Either);
    assert_eq!(keying("MasterSelectors"), Keying::Either, "a section no spec lists");

    let both = Row { name: Some(Arc::from("Row_1")), ix: Some(4), ..Row::default() };
    assert_eq!(both.key("User"), Some(RowKey::name("Row_1")), "named section: by N");
    assert_eq!(both.key("Geometry"), Some(RowKey::Ix(4)), "indexed section: by IX");
    assert_eq!(both.key("Connection"), Some(RowKey::name("Row_1")), "either: N when there is one");
    assert_eq!(Row { ix: Some(4), ..Row::default() }.key("User"), Some(RowKey::Ix(4)), "no N: IX");
    assert_eq!(Row::default().key("User"), None);
}

#[test]
fn a_name_that_differs_only_by_case_is_a_row_of_its_own() {
    assert_ne!(RowKey::name("Prop1"), RowKey::name("prop1"));
}

#[test]
fn names_that_are_empty_or_enormous_are_refused_at_the_write() {
    let mut sheet = Sheet::default();
    assert_eq!(sheet.set_cell(&CellAddr::direct(""), num("1")), Err(DocError::BadName("a cell name")));
    let long = "x".repeat(diacraft_doc::MAX_NAME_BYTES + 1);
    assert!(matches!(sheet.set_cell(&CellAddr::direct(&long), num("1")), Err(DocError::BadName(_))));
    assert!(matches!(sheet.set_cell(&CellAddr::in_row(&long, None, RowKey::Ix(0), "X"), num("1")), Err(DocError::BadName(_))));
    assert!(matches!(sheet.set_cell(&CellAddr::in_row("User", None, RowKey::name(""), "X"), num("1")), Err(DocError::BadName(_))));
    assert!(sheet.sections.is_empty() && sheet.cells.is_empty(), "a refused write changes nothing");
}

#[test]
fn a_sheet_has_limits_that_a_write_cannot_pass() {
    let mut sheet = Sheet::default();
    for i in 0..MAX_CELLS_PER_CONTAINER {
        sheet.set_cell(&CellAddr::direct(&format!("c{i}")), num("1")).unwrap();
    }
    assert!(matches!(sheet.set_cell(&CellAddr::direct("one more"), num("1")), Err(DocError::Limit { .. })));
    // Replacing an existing cell is not adding one.
    sheet.set_cell(&CellAddr::direct("c0"), num("2")).unwrap();

    let mut sheet = Sheet::default();
    for i in 0..MAX_SECTIONS_PER_SHEET {
        sheet.set_cell(&CellAddr::in_section("Geometry", Some(u32::try_from(i).unwrap()), "NoFill"), num("1")).unwrap();
    }
    let e = sheet.set_cell(&CellAddr::in_section("Geometry", Some(u32::MAX), "NoFill"), num("1")).unwrap_err();
    assert!(matches!(e, DocError::Limit { .. }), "{e:?}");

    let mut sheet = Sheet::default();
    for i in 0..MAX_ROWS_PER_SECTION {
        sheet.set_cell(&CellAddr::in_row("Scratch", None, RowKey::Ix(u32::try_from(i).unwrap()), "X"), num("1")).unwrap();
    }
    let e = sheet.set_cell(&CellAddr::in_row("Scratch", None, RowKey::Ix(u32::MAX), "X"), num("1")).unwrap_err();
    assert!(matches!(e, DocError::Limit { .. }), "{e:?}");
}

#[test]
fn duplicate_names_in_a_hostile_sheet_resolve_to_the_first() {
    // Imported sheets can hold two sections or rows with one key; both are kept, and a read takes the first.
    let mut sheet = Sheet::default();
    for v in ["first", "second"] {
        let mut s = Section::new("User", None);
        let mut r = Row { name: Some(Arc::from("Dup")), ..Row::default() };
        r.cells.insert(CellName::new("Value"), num(v));
        s.rows.push(Arc::new(r));
        sheet.sections.push(Arc::new(s));
    }
    assert_eq!(value_of(&sheet, &CellAddr::in_row("User", None, RowKey::name("Dup"), "Value")), Some("first"));
}

#[test]
fn removing_a_cell_returns_it() {
    let mut sheet = Sheet::default();
    sheet.set_cell(&CellAddr::direct("PinX"), num("3")).unwrap();
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), num("1")).unwrap();
    assert_eq!(sheet.remove_cell(&CellAddr::direct("PinX")), Some(num("3")));
    assert_eq!(sheet.remove_cell(&CellAddr::direct("PinX")), None);
    assert_eq!(sheet.remove_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")), Some(num("1")));
    assert_eq!(sheet.remove_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(9), "X")), None);
}

#[test]
fn a_write_copies_the_row_it_touches_and_shares_the_rest() {
    let mut sheet = Sheet::default();
    for r in 0..3 {
        sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(r), "X"), num("1")).unwrap();
    }
    let before = sheet.clone();
    sheet.set_cell(&CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X"), num("5")).unwrap();
    let (a, b) = (&sheet.sections[0], &before.sections[0]);
    assert!(!Arc::ptr_eq(a, b), "the section is new");
    assert!(Arc::ptr_eq(&a.rows[0], &b.rows[0]) && Arc::ptr_eq(&a.rows[2], &b.rows[2]), "untouched rows are shared");
    assert!(!Arc::ptr_eq(&a.rows[1], &b.rows[1]));
    assert_eq!(value_of(&before, &CellAddr::in_row("Geometry", Some(0), RowKey::Ix(1), "X")), Some("1"), "the older sheet is unchanged");
}

#[test]
fn section_keys_and_names_hold_what_they_were_given() {
    let key = SectionKey::new("Geometry", Some(3));
    assert_eq!((key.name.as_str(), key.ix), ("Geometry", Some(3)));
    assert_eq!(SectionName::new("Geometry"), SectionName::new("Geometry"));
    assert_ne!(SectionName::new("geometry"), SectionName::new("Geometry"));
}
