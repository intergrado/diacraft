//! The cells a writer that writes only what differs has to write: those a reader would not rebuild by itself.

use diacraft_doc::{CellName, Cells};
use diacraft_sheetxml::delta::fits_attributes;
use diacraft_sheetxml::{CellDelta, cells_delta};

use super::{read, read_with};

fn cells(xml: &str) -> Cells {
    read(&format!("<PageSheet>{xml}</PageSheet>")).sheet.cells
}

fn names(delta: &CellDelta<'_>) -> (Vec<String>, Vec<String>) {
    (delta.changed.iter().map(|(n, _)| n.as_str().to_owned()).collect(), delta.removed.iter().map(|n| n.as_str().to_owned()).collect())
}

#[test]
fn cells_equal_to_what_the_reader_would_rebuild_are_not_written() {
    let defaults = cells(r#"<Cell N="Width" V="2" U="IN" F="Height*2"/><Cell N="Height" V="1"/>"#);
    let delta = cells_delta(&defaults, &defaults.clone());
    assert!(delta.is_empty());
    assert_eq!(names(&delta), (vec![], vec![]));
}

#[test]
fn a_cell_that_differs_in_any_one_thing_is_written() {
    let defaults = cells(r#"<Cell N="A" V="1"/><Cell N="B" V="1"/><Cell N="C" V="1"/><Cell N="D" V="1"/><Cell N="E" V="1"/><Cell N="F" V="1"/>"#);
    let now = cells(
        r#"<Cell N="A" V="2"/><Cell N="B" V="1" U="IN"/><Cell N="C" V="1" F="A+1"/><Cell N="D" V="1" E="&#35;REF!"/><Cell N="E" V="1" Odd="x"/><Cell N="F" V="1">t</Cell>"#,
    );
    let delta = cells_delta(&now, &defaults);
    assert_eq!(names(&delta), (["A", "B", "C", "D", "E", "F"].map(String::from).to_vec(), vec![]));
    // The cell written is the model's cell, not a copy of it.
    assert!(std::ptr::eq(delta.changed[0].1, now.get("A").unwrap()));
}

#[test]
fn a_cell_the_reader_would_not_rebuild_is_written_and_one_it_would_and_the_model_lacks_is_said_gone() {
    let defaults = cells(r#"<Cell N="A" V="1"/><Cell N="B" V="2"/><Cell N="C" V="3"/>"#);
    let now = cells(r#"<Cell N="A" V="1"/><Cell N="C" V="3"/><Cell N="New" V="9"/>"#);
    let delta = cells_delta(&now, &defaults);
    assert_eq!(names(&delta), (vec!["New".to_owned()], vec!["B".to_owned()]));
    assert!(!delta.is_empty());
    // Against no defaults every cell is new; against defaults and no cells every default is gone.
    assert_eq!(names(&cells_delta(&now, &Cells::new())).0.len(), 3);
    assert_eq!(names(&cells_delta(&Cells::new(), &defaults)).1, ["A", "B", "C"]);
    assert!(cells_delta(&Cells::new(), &Cells::new()).is_empty());
    // A delta that has only cells to write, or only names to say gone, is not empty.
    assert!(!cells_delta(&now, &Cells::new()).is_empty());
    assert!(!cells_delta(&Cells::new(), &defaults).is_empty());
}

#[test]
fn the_delta_is_in_the_order_of_the_names_and_a_name_is_compared_exactly() {
    let defaults = cells(r#"<Cell N="b" V="1"/>"#);
    let now = cells(r#"<Cell N="b" V="1"/><Cell N="B" V="1"/><Cell N="a" V="1"/>"#);
    let delta = cells_delta(&now, &defaults);
    assert_eq!(names(&delta).0, ["B", "a"]);
    let name = CellName::new("a");
    assert!(delta.changed.iter().any(|(n, _)| **n == name));
}

#[test]
fn only_a_cell_with_no_content_fits_the_attributes_alone() {
    let r =
        read_with::<super::Main>(r#"<PageSheet><Cell N="A" V="1"/><Cell N="B">text</Cell><Cell N="C"><x/></Cell></PageSheet>"#, super::BIG).unwrap();
    assert!(fits_attributes(r.sheet.cell("A").unwrap()));
    assert!(!fits_attributes(r.sheet.cell("B").unwrap()));
    assert!(!fits_attributes(r.sheet.cell("C").unwrap()));
}
