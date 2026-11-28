//! What reading a cell that holds a formula and no saved value costs in the readers moved to the picture's reading, counted: a page of 30,000 shapes whose
//! `LayerMember` list or container type is such a formula is worked out once per cell with one evaluator, a page that runs its steps out is read as saved from there on, and a hostile
//! row (a cycle of cells, a text too long to keep) falls back and never hangs.
//!
//! The counts are those of `diacraft_engine::draw::worked`, kept for the thread that runs the command. The timed twin of the page of 30,000 shapes is ignored.

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_engine::draw::layers::page_layers;
use diacraft_engine::draw::shapedata::read_fields;
use diacraft_engine::draw::worked::{Counts, WorkedBudget, counts, reset_counts};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{blank_session, run};

fn session_with(built: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

fn unsaved(formula: &str) -> Cell {
    Cell::with_formula(formula, None)
}

/// A page with layers 0 and 1 and `n` shapes whose `LayerMember` is `formula` and no saved value.
fn page_with_lists(n: u32, formula: &str) -> Document {
    let at = |ix: u32| CellAddr::in_row("Layer", None, RowKey::Ix(ix), "Name");
    let mut page = PageBuilder::new()
        .cell("PageWidth", "400")
        .cell("PageHeight", "400")
        .set(at(0), Cell::constant(diacraft_doc::Value::from_text("Back")))
        .set(at(1), Cell::constant(diacraft_doc::Value::from_text("Front")));
    for id in 1..=n {
        page = page.shape(ShapeBuilder::new(id).set(CellAddr::direct("LayerMember"), unsaved(formula)));
    }
    DocBuilder::new().page(1, page).build()
}

fn shapes_on(doc: &Document, layer: usize) -> usize {
    page_layers(doc, 1).unwrap()[layer].shapes
}

#[test]
fn a_page_of_thirty_thousand_lists_that_are_formulas_works_each_out_once_with_one_evaluator() {
    let doc = page_with_lists(30_000, "0+1");
    let budget = WorkedBudget::new(diacraft_engine::draw::worked::WORKED_STEPS);
    let _scope = budget.enter();
    reset_counts();
    let on_front = shapes_on(&doc, 1);
    let c = counts();
    eprintln!("30,000 lists: {c:?}");
    assert_eq!(on_front, 30_000, "every list is the layer the formula says");
    assert_eq!(c.worked_out, 30_000, "each cell was worked out once: {c:?}");
    assert_eq!((c.unanswered, c.evaluators), (0, 1), "{c:?}");
}

#[test]
fn a_page_of_thirty_thousand_lists_whose_formula_fails_is_on_no_layer_and_costs_the_same_work() {
    let doc = page_with_lists(30_000, "1/0");
    let budget = WorkedBudget::new(diacraft_engine::draw::worked::WORKED_STEPS);
    let _scope = budget.enter();
    reset_counts();
    let layers = page_layers(&doc, 1).unwrap();
    assert_eq!((layers[0].shapes, layers[1].shapes), (0, 0));
    let c: Counts = counts();
    assert_eq!(c.worked_out, 30_000, "each cell once: {c:?}");
    assert_eq!(c.evaluators, 1, "{c:?}");
}

#[test]
fn a_page_whose_lists_cost_more_than_the_steps_of_its_budget_is_read_as_saved_from_there_on() {
    // `0+1` is some eight steps, so 30,000 are some 240,000: under a budget of 5,000 the first are answered and the rest are on no layer, and nothing hangs.
    let doc = page_with_lists(30_000, "0+1");
    let budget = WorkedBudget::new(5_000);
    let _scope = budget.enter();
    reset_counts();
    let on_front = shapes_on(&doc, 1);
    let c = counts();
    assert!(on_front > 100 && on_front < 30_000, "some lists were answered, not all: {on_front}");
    assert!(c.unanswered > 0 && c.steps <= 5_000, "the rest were left and counted, and the steps are the budget's: {c:?}");
}

#[test]
fn a_container_page_of_thirty_thousand_types_that_are_formulas_works_each_out_once_with_one_evaluator() {
    let user = |name: &str| CellAddr::in_row("User", None, RowKey::name(name), "Value");
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for id in 1..=30_000u32 {
        page = page.shape(
            ShapeBuilder::new(id)
                .cell("PinX", &f64::from(id % 200).to_string())
                .cell("PinY", &f64::from(id / 200).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .set(user("msvStructureType"), unsaved("\"Con\"&\"tainer\"")),
        );
    }
    let mut s = session_with(DocBuilder::new().page(1, page).build());
    reset_counts();
    let info = run(&mut s, "container.info", json!({"ids": [1], "page": 1}));
    let c = counts();
    eprintln!("30,000 container types: {c:?}");
    assert_eq!(info["containers"][0]["id"], json!(1), "{info}");
    assert!(
        c.worked_out >= 30_000 && c.worked_out <= 31_000,
        "each type was worked out once, and the few cells the command reads of the shape: {c:?}"
    );
    assert!(c.evaluators <= 2, "one evaluator for the page: {c:?}");
}

#[test]
fn a_row_whose_cells_read_each_other_in_a_cycle_falls_back_and_never_hangs() {
    let prop = |name: &str, cell: &str| CellAddr::in_row("Property", None, RowKey::name(name), cell);
    let built = DocBuilder::new()
        .page(
            1,
            PageBuilder::new().shape(
                ShapeBuilder::new(1)
                    .set(prop("A", "Label"), unsaved("Prop.B.Label"))
                    .set(prop("B", "Label"), unsaved("Prop.A.Label"))
                    .set(prop("A", "Value"), unsaved("Prop.A.Value+1")),
            ),
        )
        .build();
    reset_counts();
    let listed = read_fields(&built, 1, 1).unwrap();
    assert_eq!(listed.fields.len(), 2);
    assert!(listed.fields.iter().all(|f| f.label.is_empty()), "a cycle is no label: {:?}", listed.fields);
    assert!(counts().worked_out < 100, "the cycle was cut and not run for its steps: {:?}", counts());
}

#[test]
fn a_value_that_is_a_text_too_long_to_keep_is_read_as_a_value_that_holds_none() {
    let long = "a".repeat(100_000);
    let prop = |cell: &str| CellAddr::in_row("Property", None, RowKey::name("Note"), cell);
    let built = DocBuilder::new()
        .page(
            1,
            PageBuilder::new()
                .shape(ShapeBuilder::new(1).set(prop("Value"), unsaved(&format!("\"{long}\""))).set(prop("Label"), unsaved("\"Note\""))),
        )
        .build();
    let listed = read_fields(&built, 1, 1).unwrap();
    assert_eq!(listed.fields[0].label, "Note", "the row's other cells are read");
    assert_eq!(listed.fields[0].value, "", "a text past the most a reader keeps is no value, as a formula that fails is none");
}

#[test]
#[ignore = "timed: a page of 30,000 lists that are formulas; the counted tests above hold the work, this the time"]
fn timed_a_page_of_thirty_thousand_lists_that_are_formulas_is_read_in_well_under_a_second() {
    let doc = page_with_lists(30_000, "0+1");
    let budget = WorkedBudget::new(diacraft_engine::draw::worked::WORKED_STEPS);
    let _scope = budget.enter();
    let started = std::time::Instant::now();
    let on_front = shapes_on(&doc, 1);
    let took = started.elapsed();
    eprintln!("30,000 lists in {took:?}");
    assert_eq!(on_front, 30_000);
}
