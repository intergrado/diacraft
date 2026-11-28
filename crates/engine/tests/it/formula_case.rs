//! A cell the catalogue lists is named in any letter case in a formula: `User.Score = WIDTH/1 in*12` with a Width of 4 inches is 48, as the reference application read it on the
//! project's test PC (2026-11-27). Before, `WIDTH` named a cell no shape holds and read 0, and nothing worked the formula out again when `Width` changed.

use diacraft_doc::Document;
use serde_json::json;

use crate::edit::{doc, drop_shape, num};
use crate::{blank_session, check_session, run};

/// A rectangle with a `User.Score` row whose formula is `formula`. → the session and the shape.
fn scored(formula: &str) -> (diacraft_engine::Session, u32) {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 4.0, 4.0);
    run(&mut s, "sheet.add_section", json!({"id": id, "page": 1, "section": "User"}));
    run(&mut s, "sheet.add_row", json!({"id": id, "page": 1, "section": "User", "name": "Score"}));
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "User.Score.Value", "formula": formula}));
    (s, id)
}

fn score(s: &mut diacraft_engine::Session, id: u32) -> f64 {
    run(s, "sheet.cell", json!({"id": id, "page": 1, "cell": "User.Score.Value"}))["value"].as_f64().unwrap()
}

#[test]
fn a_formula_that_names_width_in_capitals_reads_the_width_and_follows_it() {
    let (mut s, id) = scored("WIDTH/1 in*12");
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Width", "formula": "4 in"}));
    assert!((score(&mut s, id) - 48.0).abs() < 1.0e-9, "Width 4 in");
    // Worked out again when the cell it names changes, by a formula and by a resize.
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "Width", "formula": "5 in"}));
    assert!((score(&mut s, id) - 60.0).abs() < 1.0e-9, "Width 5 in");
    run(&mut s, "shape.resize", json!({"id": id, "x": 3.0, "y": 3.0, "width": 2.5, "height": 1.0}));
    assert!((num(doc(&s), id, "Width") - 2.5).abs() < 1.0e-9);
    assert!((score(&mut s, id) - 30.0).abs() < 1.0e-9, "Width 2.5 in");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn the_formula_is_kept_as_it_was_typed() {
    let (mut s, id) = scored("WIDTH/1 in*12");
    let cell = run(&mut s, "sheet.cell", json!({"id": id, "page": 1, "cell": "User.Score.Value"}));
    assert_eq!(cell["formula"], "WIDTH/1 in*12");
}

#[test]
fn other_cells_and_other_cases_read_the_cell_too_and_a_name_the_catalogue_does_not_list_stays_exact() {
    let (mut s, id) = scored("PINX+HeIgHt");
    let (x, h) = (num(doc(&s), id, "PinX"), num(doc(&s), id, "Height"));
    assert!((score(&mut s, id) - (x + h)).abs() < 1.0e-9, "PINX+HeIgHt");
    // A name that is no cell of the catalogue is read as it is spelled: two cells that differ by case are two.
    let (mut s, id) = scored("Mine+MINE");
    assert!(score(&mut s, id).abs() < 1.0e-9, "no cell of either name holds a number");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_cell_named_in_a_command_in_another_case_is_the_cell() {
    let mut s = blank_session();
    let id = drop_shape(&mut s, "rectangle", 4.0, 4.0);
    run(&mut s, "sheet.set_cell", json!({"id": id, "page": 1, "cell": "WIDTH", "formula": "3 in"}));
    let d: &Document = doc(&s);
    assert!((num(d, id, "Width") - 3.0).abs() < 1.0e-9, "the write went to Width");
    let shape = d.shape(&d.find_shape(diacraft_doc::Owner::Page(1), id).unwrap()).unwrap();
    assert!(shape.sheet.cell("WIDTH").is_none(), "no cell called WIDTH was made");
    let answer = run(&mut s, "sheet.cell", json!({"id": id, "page": 1, "cell": "width"}));
    assert_eq!(answer["ref"], "Width", "{answer}");
    assert_eq!(answer["value"], json!(3.0), "{answer}");
}
