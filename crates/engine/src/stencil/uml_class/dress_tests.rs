//! What a dress may say beyond its other fields: row cells (`dress::RowCell`). A cell of a row of a section is set by a number or a formula; the section and the row are made when
//! the dress has none of them, a row the dress made keeps its other cells, and a list is read no further than [`MAX_ROW_CELLS`].
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::{CellName, FormulaSlot, Row, Section};

use super::super::dress::{CellSet, MAX_ROW_CELLS, RowCell};
use super::super::{CharRow, Dress, MasterSpec, More, Outline, TextSpec, build_master};

/// A dress with a `Character` row of its own, a formula in that row's `Style`, and cells of a section it makes: rows 0 and 2 named in the order 0, 2, 1.
static DRESS: Dress = Dress {
    text: TextSpec::Plain("x"),
    chars: &[CharRow { size_pt: 11.0, style: 1, color: None }],
    rows: &[
        RowCell { section: "Character", ix: 0, cell: "Style", set: CellSet::Formula("IF(1,3,1)") },
        RowCell { section: "Paragraph", ix: 0, cell: "HorzAlign", set: CellSet::Number(0.0) },
        RowCell { section: "Paragraph", ix: 2, cell: "HorzAlign", set: CellSet::Number(2.0) },
        RowCell { section: "Paragraph", ix: 1, cell: "SpAfter", set: CellSet::Number(0.1) },
    ],
    ..Dress::NONE
};
static MORE: More = More { dress: Some(&DRESS), hidden: false, brings: &[] };

/// Forty row cells, one for each of forty rows of a section.
const fn forty() -> [RowCell; 40] {
    let mut out = [RowCell { section: "Paragraph", ix: 0, cell: "SpAfter", set: CellSet::Number(1.0) }; 40];
    let mut i = 0;
    while i < 40 {
        out[i].ix = i as u32;
        i += 1;
    }
    out
}
static FORTY: [RowCell; 40] = forty();
static FORTY_DRESS: Dress = Dress { rows: &FORTY, ..Dress::NONE };
static FORTY_MORE: More = More { dress: Some(&FORTY_DRESS), hidden: false, brings: &[] };

/// Twenty names of one cell of one row.
static SAME: [RowCell; 20] = [RowCell { section: "Paragraph", ix: 0, cell: "SpAfter", set: CellSet::Number(1.0) }; 20];
static SAME_DRESS: Dress = Dress { rows: &SAME, ..Dress::NONE };
static SAME_MORE: More = More { dress: Some(&SAME_DRESS), hidden: false, brings: &[] };

fn spec_with(more: &'static More) -> MasterSpec {
    MasterSpec { id: "t.rows", name: "Rows", width: 1.0, height: 1.0, identity: (200, 80), outline: Outline::Ellipse, points: &[], more }
}

fn number(row: &Row, cell: &str) -> Option<f64> {
    row.cells.get(&CellName::new(cell)).and_then(|c| c.value.as_ref()?.number())
}

fn section(master: &diacraft_doc::Master, name: &str) -> Section {
    let shape = master.shapes.get(0).unwrap();
    shape.sheet.sections.iter().find(|s| s.name.as_str() == name).map(|s| (**s).clone()).unwrap_or_else(|| panic!("no {name} section"))
}

#[test]
fn a_dress_row_cell_sets_one_cell_of_a_row_and_makes_the_section_and_the_row_when_there_is_none() {
    let master = build_master(&spec_with(&MORE), 1);
    // The `Character` row the dress made keeps its size, and its `Style` is the formula, which the master's build settled to what it gives.
    let character = section(&master, "Character");
    assert_eq!(character.rows.len(), 1, "no row is added beside the one the dress made");
    let row = character.rows.first().unwrap();
    assert!((number(row, "Size").unwrap() - 11.0 / 72.0).abs() < 1e-9, "its other cells stay");
    let style = row.cells.get(&CellName::new("Style")).unwrap();
    assert!(matches!(&style.formula, FormulaSlot::Local(f) if f.source.as_ref() == "IF(1,3,1)"), "the formula replaced the constant");
    assert_eq!(number(row, "Style"), Some(3.0));
    // A section the dress had none of is made once, its rows in the order of their `IX`, each with its cell.
    let paragraph = section(&master, "Paragraph");
    assert_eq!(paragraph.rows.iter().map(|r| r.ix).collect::<Vec<_>>(), [Some(0), Some(1), Some(2)]);
    assert_eq!(number(paragraph.rows.first().unwrap(), "HorzAlign"), Some(0.0));
    assert_eq!(number(paragraph.rows.get(1).unwrap(), "SpAfter"), Some(0.1));
    assert_eq!(number(paragraph.rows.get(2).unwrap(), "HorzAlign"), Some(2.0));
    let shape = master.shapes.get(0).unwrap();
    assert_eq!(shape.sheet.sections.iter().filter(|s| s.name.as_str() == "Paragraph").count(), 1);
}

#[test]
fn a_dress_sets_at_most_sixteen_row_cells_and_names_of_one_cell_make_one() {
    // Forty row cells for forty rows: the first sixteen are read and the rest are not.
    let master = build_master(&spec_with(&FORTY_MORE), 1);
    assert_eq!(section(&master, "Paragraph").rows.len(), MAX_ROW_CELLS);
    // The same cell named twenty times is one row with one cell.
    let master = build_master(&spec_with(&SAME_MORE), 1);
    let paragraph = section(&master, "Paragraph");
    assert_eq!((paragraph.rows.len(), paragraph.rows.first().unwrap().cells.len()), (1, 1));
}
