//! A cell given as attributes: the form a cell takes where it is no element of its own (a tier of DiaCraft's data in an `.odg` shape).

use diacraft_doc::{Cells, FormulaSlot};
use diacraft_sheetxml::build::cell_attrs;
use diacraft_sheetxml::{Cx, LimitKind, SheetReader, Warnings};

use super::{BIG, read};

const ATTRS: [(&str, &str); 5] = [("V", "1.50"), ("U", "IN"), ("F", "B+1"), ("E", "#REF!"), ("X", "y")];

fn put(
    reader: &mut SheetReader,
    warnings: &mut Warnings,
    cells: &mut Cells,
    name: &str,
    attrs: &[(&str, &str)],
) -> Result<bool, diacraft_sheetxml::LimitError> {
    let mut cx = Cx::new(warnings, "/content.xml");
    reader.put_cell_attrs(&mut cx, cells, name, attrs)
}

#[test]
fn a_cell_given_as_attributes_is_the_cell_the_element_would_have_been_and_costs_the_same() {
    let element = read(r#"<PageSheet><Cell N="A" V="1.50" U="IN" F="B+1" E="&#35;REF!" X="y"/></PageSheet>"#);
    let mut reader = SheetReader::new(BIG);
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &ATTRS).unwrap());
    assert_eq!(cells.get("A"), element.sheet.cell("A"));
    let cell = cells.get("A").unwrap();
    assert_eq!(cell.value.as_ref().unwrap().text(), "1.50");
    assert_eq!(cell.formula.text(), Some("B+1"));
    assert_eq!(cell.raw.get("X"), Some("y"));
    // The element costs 1 + 6 for the cell and the sheet's own 1; the attributes cost the same 7.
    let (e, a) = (element.reader.stats(), reader.stats());
    assert_eq!((a.cells, a.items), (1, 7));
    assert_eq!((e.cells, e.items), (1, 8));
    assert_eq!((a.formula_nodes, a.formula_cells), (e.formula_nodes, e.formula_cells));
    assert!(warnings.messages.is_empty());
}

#[test]
fn a_name_that_is_taken_or_empty_stores_nothing_says_so_and_is_paid_for_as_a_cell() {
    let mut reader = SheetReader::new(BIG);
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &[("V", "1")]).unwrap());
    assert!(!put(&mut reader, &mut warnings, &mut cells, "A", &[("V", "2")]).unwrap());
    assert!(!put(&mut reader, &mut warnings, &mut cells, "", &[("V", "3")]).unwrap());
    // The first of a name is the one read.
    assert_eq!(cells.len(), 1);
    assert_eq!(cells.get("A").unwrap().value.as_ref().unwrap().text(), "1");
    assert_eq!((warnings.count("duplicate_cell"), warnings.count("cell_without_name")), (1, 1));
    assert_eq!(reader.stats().cells, 3);
    // Only the cell that was stored paid for its items.
    assert_eq!(reader.stats().items, 3);
}

#[test]
fn a_name_past_the_cap_and_a_place_with_no_room_left_are_the_errors() {
    let mut reader = SheetReader::new(BIG);
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    let long = "x".repeat(diacraft_doc::MAX_NAME_BYTES + 1);
    let e = put(&mut reader, &mut warnings, &mut cells, &long, &[]).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::NameBytes, diacraft_doc::MAX_NAME_BYTES as u64));
    for i in 0..diacraft_doc::MAX_CELLS_PER_CONTAINER {
        assert!(put(&mut reader, &mut warnings, &mut cells, &format!("c{i}"), &[]).unwrap());
    }
    let e = put(&mut reader, &mut warnings, &mut cells, "one-too-many", &[]).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::CellsInOnePlace, diacraft_doc::MAX_CELLS_PER_CONTAINER as u64));
    assert_eq!(cells.len(), diacraft_doc::MAX_CELLS_PER_CONTAINER);
}

#[test]
fn the_budgets_are_taken_before_the_cell_is_stored() {
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    let mut reader = SheetReader::new(diacraft_sheetxml::Caps { cells: 1, ..BIG });
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &[]).unwrap());
    let e = put(&mut reader, &mut warnings, &mut cells, "B", &[]).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::Cells, 1));
    assert_eq!(cells.len(), 1);
    // Items: the cell is 2 and an attribute 1, so 4 are too few for a second cell with two attributes.
    let mut reader = SheetReader::new(diacraft_sheetxml::Caps { items: 4, ..BIG });
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &[("V", "1")]).unwrap());
    let e = put(&mut reader, &mut warnings, &mut cells, "B", &[("V", "1"), ("U", "IN")]).unwrap_err();
    assert_eq!((e.kind, e.max), (LimitKind::Items, 4));
    assert_eq!(cells.len(), 1);
}

#[test]
fn a_pair_called_n_is_ignored_as_in_the_element_and_a_cell_with_a_formula_marker_is_not_a_formula() {
    let mut reader = SheetReader::new(BIG);
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &[("N", "ignored"), ("F", "Inh"), ("V", "1")]).unwrap());
    let cell = cells.get("A").unwrap();
    assert_eq!(cell.formula, FormulaSlot::Inherit);
    assert!(cell.raw.is_empty());
}

#[test]
fn what_the_attributes_of_a_cell_say_is_what_reads_back_as_that_cell() {
    let element = read(r#"<PageSheet><Cell N="A" Odd="1" F="Width*2" Del="true" V="3" U="MM"/></PageSheet>"#);
    let cell = element.sheet.cell("A").unwrap();
    let written = cell_attrs(cell);
    // The typed attributes come first, in the order `V`, `U`, `E`, `F`, then the rest as the file gave it.
    assert_eq!(written.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["V", "U", "F", "Odd", "Del"]);
    let pairs: Vec<(&str, &str)> = written.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut reader = SheetReader::new(BIG);
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &pairs).unwrap());
    assert_eq!(cells.get("A"), Some(cell));
}

#[test]
fn a_pair_called_n_costs_nothing_and_a_cell_that_is_refused_costs_the_cell_and_no_item() {
    let mut warnings = Warnings::new(10);
    let mut cells = Cells::new();
    // The same cell with and without the ignored pair: one cell and `2 + attributes` items either way.
    let mut with = SheetReader::new(BIG);
    assert!(put(&mut with, &mut warnings, &mut cells, "A", &[("N", "x"), ("V", "1"), ("N", "y"), ("U", "IN")]).unwrap());
    let mut without = SheetReader::new(BIG);
    assert!(put(&mut without, &mut warnings, &mut Cells::new(), "A", &[("V", "1"), ("U", "IN")]).unwrap());
    assert_eq!((with.stats().cells, with.stats().items), (1, 4));
    assert_eq!(with.stats(), without.stats());
    // A cell refused (a name taken, no name) is the one cell, and nothing is kept of it, so no item.
    let mut reader = SheetReader::new(BIG);
    let mut cells = Cells::new();
    assert!(put(&mut reader, &mut warnings, &mut cells, "A", &[("V", "1")]).unwrap());
    let after_one = reader.stats();
    assert!(!put(&mut reader, &mut warnings, &mut cells, "A", &[("V", "2"), ("U", "IN"), ("E", "e")]).unwrap());
    assert!(!put(&mut reader, &mut warnings, &mut cells, "", &[("V", "2"), ("U", "IN")]).unwrap());
    let after_refused = reader.stats();
    assert_eq!((after_refused.cells, after_refused.items), (after_one.cells + 2, after_one.items));
}
