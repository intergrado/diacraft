//! A `Property.Value` that gives an empty `Prompt` of a `User` row to `MAX`.
//!
//! The sample file's two cells were `#VALUE!` where the cached value is 2, once read an empty text cell of its batch as the empty
//! string (an empty cell given to `MAX` raises no error). The package here is built in code from the structure
//! the case has, with names and values of its own: one shape whose `User` section has the row `Zed` (a `Value` of 0 and an empty `Prompt`) and
//! whose `Property` section has four rows of `Type` 2, three of them as the case has them and the fourth reading the pin so that a move works it out.

use diacraft_calc::{Budget, EvalOptions, OperandMode, Scope, commit_in, recalc_all_with};
use diacraft_doc::{CellAddr, Document, Lookup, Owner, RowKey};
use diacraft_engine::Session;
use diacraft_testkit::package::{PageSpec, VsdxBuilder, shape};
use serde_json::json;

use crate::odg_build::Dir;
use crate::run;

/// The shape's cells: a pin, a size and the two sections.
fn shape_xml() -> String {
    shape(
        1,
        r#"<Cell N="PinX" V="2"/><Cell N="PinY" V="2"/><Cell N="Width" V="1"/><Cell N="Height" V="1"/>
<Section N="User"><Row N="Zed"><Cell N="Value" V="0"/><Cell N="Prompt" V=""/></Row></Section>
<Section N="Property">
<Row N="P1"><Cell N="Type" V="2"/><Cell N="Value" V="2" F="MAX(2,User.Zed.Prompt)"/></Row>
<Row N="P2"><Cell N="Type" V="2"/><Cell N="Value" V="2" F="ROUND(MAX(User.Zed.Prompt,1.5),0)"/></Row>
<Row N="C1"><Cell N="Type" V="2"/><Cell N="Value" V="2" F="MAX(2,User.Zed)"/></Row>
<Row N="P3"><Cell N="Type" V="2"/><Cell N="Value" V="2" F="MAX(PinX,User.Zed.Prompt)"/></Row>
</Section>"#,
    )
}

fn opened(dir: &Dir) -> Session {
    let bytes = VsdxBuilder::new().page(PageSpec::new(1, "Page-1").shapes(&shape_xml())).build();
    let path = dir.write("in.vsdx", &bytes);
    let mut s = Session::new();
    run(&mut s, "file.open", json!({"path": path}));
    s
}

/// The cached value and the error of `Property.<row>.Value` of shape 1.
fn property(doc: &Document, row: &str) -> (Option<String>, Option<String>) {
    let at = doc.find_shape(Owner::Page(1), 1).unwrap();
    let Lookup::Found(cell) = doc.shape(&at).unwrap().sheet.lookup(&CellAddr::in_row("Property", None, RowKey::name(row), "Value")) else {
        panic!("Property.{row}.Value is in the file");
    };
    (cell.value.as_ref().map(|v| v.text().to_owned()), cell.error.as_deref().map(str::to_owned))
}

#[test]
fn an_empty_prompt_given_to_max_scores_as_the_file_saved_it() {
    let dir = Dir::new();
    let s = opened(&dir);
    let options = EvalOptions { operands: OperandMode::Cached, ..EvalOptions::default() };
    let report = recalc_all_with(s.document().unwrap(), &Budget::default(), options);
    assert!(report.differed.is_empty(), "{:?}", report.differed);
    // The four rows are formula cells and all four agree: the two cells of the case, its control and the row that reads the pin.
    let max = report.by_function.get("MAX").copied().unwrap_or_default();
    assert_eq!((max.agreed, max.differed), (4, 0), "{:?}", report.by_function);
}

#[test]
fn a_page_recalculation_writes_no_error_for_an_empty_prompt_given_to_max() {
    let dir = Dir::new();
    let s = opened(&dir);
    let mut doc = s.document().unwrap().clone();
    commit_in(&mut doc, &Budget::default(), EvalOptions::default(), Scope::Page(1));
    for row in ["P1", "P2", "C1", "P3"] {
        assert_eq!(property(&doc, row), (Some("2".to_owned()), None), "Property.{row}.Value");
    }
}

#[test]
fn moving_the_shape_recalculates_a_max_that_gives_an_empty_prompt_without_an_error() {
    let dir = Dir::new();
    let mut s = opened(&dir);
    run(&mut s, "shape.move", json!({"ids": [1], "dx": 1.0, "dy": 0.0}));
    let doc = s.document().unwrap();
    // The pin is 3 now, and 3 is above the empty cell's 0; the rows that do not read the pin are as they were.
    assert_eq!(property(doc, "P3"), (Some("3".to_owned()), None), "the row that reads the pin");
    for row in ["P1", "P2", "C1"] {
        assert_eq!(property(doc, row), (Some("2".to_owned()), None), "Property.{row}.Value");
    }
}
