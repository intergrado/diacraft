//! The stamp of a paragraph's or a run's row (`diacraft_doc::stamp_text_row`, the `stamp` of an `OdfTextStyle`) follows the splice writer's
//! **key rule**, not `cell_same`'s: two rows are one to a save when their keys are, `name=text|unit;` for each cell in order, the value's text and the unit
//! verbatim (`Props::key`, and `key_of` in `View::same`; "a row is compared by its cells, never by its `IX`"). So `0.2` and `0.20`, `1` and
//! `1.0`, a unit and none are different rows, where `cell_same` takes each pair for one value.

use diacraft_doc::{Cell, CellName, FormulaSlot, Row, Sheet, Value, stamp_text_row};

use super::text_stamp_agrees::{paragraph_cells, rows_of, run_cells};
use super::{no_palette, row_keys, row_props};

/// A row of the cells named, each a constant: `(name, text, unit)`.
fn row_of(cells: &[(&str, &str, &str)]) -> Row {
    let mut row = Row::default();
    for (name, value, unit) in cells {
        let cell = Cell::constant(Value::from_text(value));
        row.cells.insert(CellName::new(name), if unit.is_empty() { cell } else { cell.with_unit(unit) });
    }
    row
}

/// The key the splice writer makes of a row: that of `row_props`, or none for a row with no cell.
fn key_of(row: &Row, paragraph: bool) -> String {
    match row_props(row, paragraph, &no_palette) {
        Ok(Some(props)) => props.key.clone(),
        Ok(None) => String::new(),
        Err(e) => panic!("{e}: {row:?}"),
    }
}

#[test]
fn the_pairs_the_review_named_are_different_rows_to_the_stamp_as_to_the_key() {
    type Cells<'a> = &'a [(&'a str, &'a str, &'a str)];
    let pairs: [(Cells, Cells, bool); 8] = [
        (&[("Size", "0.2", "")], &[("Size", "0.20", "")], false),
        (&[("Size", "0.2", "")], &[("Size", "0.2", "PT")], false),
        (&[("Style", "1", "")], &[("Style", "1.0", "")], false),
        (&[("HorzAlign", "1", "")], &[("HorzAlign", "1.0", "")], true),
        (&[("IndLeft", "0.5", "IN")], &[("IndLeft", "0.5", "")], true),
        (&[("IndLeft", "0.5", "IN")], &[("IndLeft", "0.5", "CM")], true),
        (&[("Letterspace", "0", "")], &[("Letterspace", "0.0", "")], false),
        (&[("Color", "#112233", "")], &[("Color", "#112233", "X")], false),
    ];
    for (a, b, paragraph) in pairs {
        let (a, b) = (row_of(a), row_of(b));
        assert_ne!(key_of(&a, paragraph), key_of(&b, paragraph), "{a:?} and {b:?}: the splice writer tells them apart");
        assert_ne!(stamp_text_row(&a), stamp_text_row(&b), "{a:?} and {b:?}");
    }
}

#[test]
fn what_is_no_part_of_the_key_is_no_part_of_the_stamp() {
    // The row's `IX`, its `N`, its type, its flag and its other attributes are not in the key.
    let base = row_of(&[("Size", "0.2", ""), ("Style", "1", "")]);
    let mut other = base.clone();
    other.ix = Some(7);
    other.name = Some("n".into());
    other.row_type = Some("t".into());
    other.del = true;
    other.raw = diacraft_doc::RawAttrs::from_pairs([("a", "b")]);
    assert_eq!(key_of(&base, false), key_of(&other, false));
    assert_eq!(stamp_text_row(&base), stamp_text_row(&other));
    // A cell's formula slot of a constant (`No Formula`, none) is not either: the value is.
    let mut slot = base.clone();
    if let Some(cell) = slot.cells.get_mut("Size") {
        cell.formula = FormulaSlot::NoFormula;
    }
    assert_eq!(key_of(&base, false), key_of(&slot, false));
    assert_eq!(stamp_text_row(&base), stamp_text_row(&slot));
    // And the order of the cells is the map's, the same in both.
    assert_eq!(stamp_text_row(&row_of(&[("Style", "1", ""), ("Size", "0.2", "")])), stamp_text_row(&base));
}

fn compare(cells: &[(&'static str, Vec<(&'static str, &'static str)>)], paragraph: bool) -> (usize, usize) {
    let rows = rows_of(cells);
    let keys: Vec<String> = rows.iter().map(|row| key_of(row, paragraph)).collect();
    let stamps: Vec<_> = rows.iter().map(stamp_text_row).collect();
    let (mut same, mut different) = (0, 0);
    for i in 0..rows.len() {
        for j in 0..rows.len() {
            assert_eq!(stamps[i] == stamps[j], keys[i] == keys[j], "{:?} ({:?}) and {:?} ({:?})", rows[i], keys[i], rows[j], keys[j]);
            if keys[i] == keys[j] {
                same += 1;
            } else {
                different += 1;
            }
        }
    }
    (same, different)
}

#[test]
fn a_run_rows_stamp_says_what_its_key_says_over_every_pair_of_rows() {
    let (same, different) = compare(&run_cells(), false);
    assert!(same > 100 && different > 10_000, "{same} the same, {different} different");
}

#[test]
fn a_paragraph_rows_stamp_says_what_its_key_says_over_every_pair_of_rows() {
    let (same, different) = compare(&paragraph_cells(), true);
    assert!(same > 100 && different > 10_000, "{same} the same, {different} different");
}

#[test]
fn a_row_with_no_cell_has_the_key_of_none_and_a_row_the_writer_refuses_has_a_key_of_its_own() {
    // No cell is no row: its key is the empty text, and it stamps as every row with no cell does.
    assert_eq!(key_of(&Row::default(), false), "");
    assert_eq!(stamp_text_row(&Row::default()), stamp_text_row(&Row { ix: Some(3), del: true, ..Row::default() }));
    assert_ne!(stamp_text_row(&Row::default()), stamp_text_row(&row_of(&[("Size", "", "")])));
    // A cell the writer cannot read as a value (a formula, no value) is `name=?;` in `row_keys`, whatever it holds: the rows stamp alike exactly when those do.
    let mut rows: Vec<Row> = Vec::new();
    for cell in [
        Cell::with_formula("Size*2", Some(Value::from_text("1"))),
        Cell::with_formula("Size*3", Some(Value::from_text("1"))),
        Cell::inherited(Some(Value::from_text("1"))),
        Cell::new(),
        Cell::constant(Value::from_text("1")),
        Cell::constant(Value::from_text("2")),
    ] {
        let mut row = row_of(&[("Style", "1", "")]);
        row.cells.insert(CellName::new("Size"), cell);
        rows.push(row);
    }
    let key = |row: &Row| {
        let mut sheet = Sheet::default();
        let mut section = diacraft_doc::Section::new("Character", None);
        section.rows.push(std::sync::Arc::new(Row { ix: Some(0), ..row.clone() }));
        sheet.sections.push(std::sync::Arc::new(section));
        row_keys(&sheet, &mut 0).into_iter().next().map(|(_, key)| key)
    };
    for a in &rows {
        for b in &rows {
            assert_eq!(stamp_text_row(a) == stamp_text_row(b), key(a) == key(b), "{a:?} and {b:?}");
        }
    }
}
