//! A text style's stamp (`diacraft_doc::stamp_text_group`) tells two rows apart exactly where the splice writer's `Part::sig` does, group by
//! group (the groups and signatures of [`owned`] and [`Part`]). So the model path keeps a held style
//! value while the splice writer keeps it, and drops it when the splice writer writes the group again: the alignment by `HorzAlign` and `Flags`, the underline
//! by bit 4 of `Style` and `DblUnderline`, and a cell by its value's text and its unit.
//!
//! The rows are every one-cell change of a base row (each other value of the cell, a respelling, a unit, and the cell gone) and 300 rows of random cells, and
//! the two are compared pair by pair over all of them.

use diacraft_doc::{Cell, CellName, Row, TextGroup, Value, stamp_text_group};

use super::{no_palette, owned, row_props};

/// One cell: its name and the values (text, unit) it takes.
pub(super) type Options = (&'static str, Vec<(&'static str, &'static str)>);

pub(super) fn run_cells() -> Vec<Options> {
    vec![
        ("Size", vec![("0.1666667", ""), ("0.2", ""), ("0.1666667", "IN"), ("0.20", "")]),
        (
            "Style",
            vec![("0", ""), ("1", ""), ("2", ""), ("4", ""), ("5", ""), ("5.0", ""), ("7", ""), ("8", ""), ("63", ""), ("1", "IN"), ("4", "IN")],
        ),
        ("DblUnderline", vec![("0", ""), ("1", ""), ("0.0", ""), ("0", "IN")]),
        ("Strikethru", vec![("0", ""), ("1", ""), ("0.0", "")]),
        ("DoubleStrikethrough", vec![("0", ""), ("1", ""), ("1", "IN")]),
        ("Overline", vec![("0", ""), ("1", ""), ("1", "IN")]),
        ("Case", vec![("0", ""), ("1", ""), ("2", ""), ("1", "IN")]),
        ("Pos", vec![("0", ""), ("1", ""), ("2", ""), ("2", "IN")]),
        ("Color", vec![("#112233", ""), ("#AABBCC", ""), ("#112233", "X")]),
        ("Letterspace", vec![("0", ""), ("0.01", ""), ("0.010", "")]),
        ("FontScale", vec![("1", ""), ("1.5", ""), ("1.50", "")]),
        ("Font", vec![("0", ""), ("Arial", ""), ("Noto Sans", "")]),
        ("AsianFont", vec![("0", ""), ("SimSun", ""), ("Arial", "")]),
        ("ComplexScriptFont", vec![("0", ""), ("Tahoma", ""), ("Arial", "")]),
    ]
}

pub(super) fn paragraph_cells() -> Vec<Options> {
    vec![
        ("HorzAlign", vec![("0", ""), ("1", ""), ("2", ""), ("3", ""), ("4", ""), ("1", "X")]),
        ("Flags", vec![("0", ""), ("1", ""), ("0", "X")]),
        ("IndLeft", vec![("0", ""), ("0.5", ""), ("0.50", ""), ("0.5", "IN")]),
        ("IndRight", vec![("0", ""), ("0.5", ""), ("0.5", "IN")]),
        ("IndFirst", vec![("0", ""), ("-0.25", ""), ("-0.25", "IN")]),
        ("SpBefore", vec![("0", ""), ("0.1", ""), ("0.1", "IN")]),
        ("SpAfter", vec![("0", ""), ("0.1", ""), ("0.1", "IN")]),
        ("SpLine", vec![("1", ""), ("0", ""), ("-1.5", ""), ("1.2", ""), ("1.2", "IN")]),
    ]
}

/// A row from the choice made for each cell (`None`: the cell is not there).
fn row_from(cells: &[Options], choice: &[Option<usize>]) -> Row {
    let mut row = Row::default();
    for ((name, options), pick) in cells.iter().zip(choice) {
        if let Some((text, unit)) = pick.and_then(|at| options.get(at)) {
            let cell = Cell::constant(Value::from_text(text));
            row.cells.insert(CellName::new(name), if unit.is_empty() { cell } else { cell.with_unit(unit) });
        }
    }
    row
}

/// The rows to compare: every one-cell change of the base row, and 300 of random cells.
pub(super) fn rows_of(cells: &[Options]) -> Vec<Row> {
    let base: Vec<Option<usize>> = vec![Some(0); cells.len()];
    let mut choices: Vec<Vec<Option<usize>>> = vec![base.clone()];
    for (at, (_, options)) in cells.iter().enumerate() {
        for pick in (0..options.len()).map(Some).chain([None]) {
            let mut choice = base.clone();
            choice[at] = pick;
            choices.push(choice);
        }
    }
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = |modulus: usize| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(seed >> 33).unwrap_or(0) % modulus
    };
    for _ in 0..300 {
        choices.push(cells.iter().map(|(_, options)| Some(next(options.len() + 1)).filter(|at| *at < options.len())).collect());
    }
    choices.iter().map(|choice| row_from(cells, choice)).collect()
}

/// For every pair of rows and every group of the kind: the stamps are equal exactly when the signatures are (both absent are equal), with how often each
/// of the three cases (equal and present, different, present against absent) came up, per group.
fn compare(cells: &[Options], paragraph: bool) -> Vec<(TextGroup, usize, usize, usize)> {
    let rows = rows_of(cells);
    let props: Vec<_> = rows.iter().map(|row| row_props(row, paragraph, &no_palette).unwrap_or_else(|e| panic!("{e}: {row:?}"))).collect();
    let mut seen = Vec::new();
    for group in TextGroup::ALL.into_iter().filter(|g| g.is_paragraph() == paragraph) {
        let sig = |at: usize| props[at].as_ref().and_then(|p| p.parts.iter().find(|part| part.id == group.id()).map(|part| part.sig.clone()));
        let stamps: Vec<_> = rows.iter().map(|row| stamp_text_group(row, group)).collect();
        let (mut equal, mut different, mut against_absent) = (0, 0, 0);
        for i in 0..rows.len() {
            for j in 0..rows.len() {
                let (a, b) = (sig(i), sig(j));
                assert_eq!(stamps[i] == stamps[j], a == b, "{group:?}: {:?} ({a:?}) and {:?} ({b:?})", rows[i], rows[j]);
                match (&a, &b) {
                    (Some(_), Some(_)) if a == b => equal += 1,
                    (Some(_), Some(_)) => different += 1,
                    (Some(_), None) | (None, Some(_)) => against_absent += 1,
                    (None, None) => {}
                }
            }
        }
        seen.push((group, equal, different, against_absent));
    }
    seen
}

#[test]
fn a_run_rows_group_stamps_alike_exactly_where_the_splice_writers_signature_is_alike() {
    let seen = compare(&run_cells(), false);
    assert_eq!(seen.len(), 13);
    for (group, equal, different, against_absent) in seen {
        // Never vacuous: every group has rows it tells apart, rows it does not, and rows without it.
        assert!(equal > 0 && different > 0 && against_absent > 0, "{group:?}: {equal} equal, {different} different, {against_absent} against absent");
    }
}

#[test]
fn a_paragraph_rows_group_stamps_alike_exactly_where_the_splice_writers_signature_is_alike() {
    let seen = compare(&paragraph_cells(), true);
    assert_eq!(seen.len(), 8);
    for (group, equal, different, against_absent) in seen {
        assert!(equal > 0 && different > 0 && against_absent > 0, "{group:?}: {equal} equal, {different} different, {against_absent} against absent");
    }
}

#[test]
fn the_groups_are_the_splice_writers_names_and_the_underlines_is_the_one_made_of_two_cells() {
    // Every group the model names is one the splice writer owns attributes for: the names are `owned`'s.
    for group in TextGroup::ALL {
        assert!(!owned(group.id()).is_empty(), "{group:?} is no group of runs.rs");
    }
    // And `owned` has no group the model lacks.
    for id in [
        "Size",
        "Style",
        "Underline",
        "Strike",
        "Overline",
        "Case",
        "Pos",
        "Color",
        "Letterspace",
        "FontScale",
        "Font",
        "AsianFont",
        "ComplexScriptFont",
        "HorzAlign",
        "Flags",
        "IndLeft",
        "IndRight",
        "IndFirst",
        "SpBefore",
        "SpAfter",
        "SpLine",
    ] {
        assert!(TextGroup::ALL.iter().any(|g| g.id() == id), "{id}");
        assert!(!owned(id).is_empty(), "{id}");
    }
    // The paragraph groups are the ones `is_paragraph_group` says.
    for group in TextGroup::ALL {
        assert_eq!(group.is_paragraph(), super::is_paragraph_group(group.id()), "{group:?}");
    }
}
