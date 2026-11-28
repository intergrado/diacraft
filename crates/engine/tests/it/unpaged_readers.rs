//! The cells that read the sheets no page holds, and what a style's formula gives a shape (the last of gap F40): **a cell of the document's sheet, of a style or of a master that a write
//! worked out is read by the cells of every page that name it** (`TheDoc!`, `Styles!`, `Masters[..]!`), within the one commit of the call, and **a formula a shape inherits from a style** is listed
//! for the shape as one it inherits from a master is.
//!
//! Each is held to what no change may break (`page_readers::exact`): a full evaluation changes no cell after the command, undo and redo give the drawing back exactly, and a save and a reopen in both
//! formats give the pages the values the session has. The costs on 256 pages are **counts** (the steps the evaluator took and the commits it made), with a timed twin that is `#[ignore]`d.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_doc::{Cell, CellAddr, RowKey, Value as Held};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use serde_json::json;

use crate::page_readers::{Cost, PAGES, counted, error, exact, timed_twin, value};
use crate::sheet_cross_pages::{session_of, settled};
use crate::{check_session, run};

fn total() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("Total"), "Value")
}

/// The document's `User.Total`, worked out from the width of shape 3 of the page "Plan".
fn with_total(mut doc: diacraft_doc::Document) -> diacraft_doc::Document {
    Arc::make_mut(&mut doc.sheet).set_cell(&total(), Cell::with_formula("Pages[Plan]!Sheet.3!Width*3", Some(Held::from_text("15")))).unwrap();
    doc
}

// ---- the document's sheet ------------------------------------------------------------------------------------------------------------------------

/// Page "Plan" holds shape 3 of width 5; the document's `User.Total` is three times it; pages 2 and 3 each hold a shape whose height reads `TheDoc!User.Total`.
fn plan_total_and_two_readers() -> Session {
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+1", "16")))
        .page(3, PageBuilder::new().name("Other").shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+2", "17")))
        .build();
    session_of(with_total(doc))
}

/// A write to a cell the document's cell reads works the document's cell out, and the shapes of the other pages that read it: a page that no other cell names is reached through the
/// document's sheet.
#[test]
fn a_write_works_out_the_shapes_of_every_page_that_read_the_document_cell_it_changed() {
    let mut s = plan_total_and_two_readers();
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(run(s, "sheet.cell", json!({"document": true, "cell": "User.Total.Value"}))["value"], json!(21.0), "the document's cell");
        assert_eq!(value(s, 2, 10, "Height"), json!(22.0), "the shape of page 2");
        assert_eq!(value(s, 3, 10, "Height"), json!(23.0), "the shape of page 3");
    });
}

// ---- a style's cell, a master's cell -----------------------------------------------------------------------------------------------------------

#[test]
fn a_write_works_out_the_shapes_of_other_pages_that_read_a_style_cell_it_changed() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "Styles!Rated!LineWeight+1", "11")))
        .build();
    let mut s = session_of(doc);
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(run(s, "sheet.cell", json!({"style": 1, "cell": "LineWeight"}))["value"], json!(14.0), "the style's cell");
        assert_eq!(value(s, 2, 10, "Height"), json!(15.0), "the shape that reads it");
    });
}

/// A master's shape whose width reads a page; a shape of page 2, which holds no instance of the master, reads it by the master's name.
#[test]
fn a_write_works_out_the_shapes_of_other_pages_that_read_a_master_cell_it_changed() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "Masters[Meter]!Sheet.5!Width+1", "11")))
        .build();
    let mut s = session_of(doc);
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(run(s, "sheet.cell", json!({"master": 1, "id": 5, "cell": "Width"}))["value"], json!(14.0), "the master's cell");
        assert_eq!(value(s, 2, 10, "Height"), json!(15.0), "the shape that reads it");
    });
}

// ---- a formula a shape inherits from a style ----------------------------------------------------------------------------------------------------

/// Style 1 holds a line weight that reads the page "Plan"; shape 10 of page 2 is linked to it (its line style) and holds the cell as `Inh`.
fn shape_that_inherits_a_style_formula() -> Session {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).links(Some(1), None, None).inh("LineWeight", Some("10"))))
        .build();
    session_of(doc)
}

#[test]
fn a_write_works_out_the_shape_that_inherits_a_style_formula_that_names_a_page() {
    let mut s = shape_that_inherits_a_style_formula();
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(value(s, 2, 10, "LineWeight"), json!(14.0), "the shape reads the page through the formula it inherits");
        assert_eq!(run(s, "sheet.cell", json!({"style": 1, "cell": "LineWeight"}))["value"], json!(14.0), "and so does the style's own cell");
    });
}

/// A page renamed is a name the formula a shape inherits from a style does not find either.
#[test]
fn renaming_a_page_works_out_the_shape_that_inherits_a_style_formula_that_names_it() {
    let s = shape_that_inherits_a_style_formula();
    let mut doc = s.document().unwrap().clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Site-1"));
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name = Arc::from("Plan");
    let mut s = session_of(doc);
    exact(&mut s, "page.rename", json!({"id": 1, "name": "Dach"}), |s| {
        assert_eq!(error(s, 2, 10, "LineWeight"), json!("#REF!"), "the shape");
        assert_eq!(run(s, "sheet.cell", json!({"style": 1, "cell": "LineWeight"}))["error"], json!("#REF!"), "and the style");
    });
}

// ---- 256 pages, counted ------------------------------------------------------------------------------------------------------------------------------

/// A timed twin is `#[ignore]`d and run by hand on a quiet machine.
fn the_costs(what: &str, first: Cost, second: Cost) {
    eprintln!("{what}: the first write {first:?}, the next {second:?}");
}

/// 256 pages: the document's `User.Total` reads the width of shape 3 of the first, and shape 10 of each of the other 255 reads the document's cell. A write to the width is **one commit** and steps that
/// are the cells' (a few hundred each), and the next write costs no more: the listing of the sheets that name the document is kept.
fn a_write_reaches_255_pages_that_read_the_document_cell() {
    let mut b = DocBuilder::new().page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")));
    for p in 2..=PAGES {
        b = b.page(p, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+1", "16")));
    }
    let mut s = session_of(with_total(b.build()));
    let (_, first) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}));
    assert_eq!(value(&mut s, PAGES, 10, "Height"), json!(22.0), "the last page reads the document's cell");
    assert_eq!(value(&mut s, 2, 10, "Height"), json!(22.0));
    settled(s.document().unwrap());
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "8"}));
    the_costs("255 pages that read the document's cell", first, second);
    assert_eq!(first.commits, 1, "one commit for the 257 cells: {first:?}");
    assert!(first.steps <= 800 * u64::from(PAGES), "a few hundred steps for each cell and no more: {first:?}");
    assert!(second.steps <= first.steps, "the listing is kept: {second:?} against {first:?}");
    assert_eq!(value(&mut s, PAGES, 10, "Height"), json!(25.0));
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_reaches_255_pages_that_read_the_document_cell_in_one_commit() {
    a_write_reaches_255_pages_that_read_the_document_cell();
}

timed_twin!(a_write_reaches_255_pages_that_read_the_document_cell_is_timed, 30, a_write_reaches_255_pages_that_read_the_document_cell);

/// 256 pages, each holding a shape linked to a style whose line weight reads the page "Plan": the write works the style's cell out and the 255 shapes in **one commit**.
fn a_write_reaches_255_shapes_that_inherit_a_style_formula() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let mut b = DocBuilder::new().style(1, rated).page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")));
    for p in 2..=PAGES {
        b = b.page(p, PageBuilder::new().shape(ShapeBuilder::new(10).links(Some(1), None, None).inh("LineWeight", Some("10"))));
    }
    let mut s = session_of(b.build());
    let (_, first) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}));
    assert_eq!(value(&mut s, PAGES, 10, "LineWeight"), json!(14.0), "the last page reads the page through the style");
    settled(s.document().unwrap());
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "8"}));
    the_costs("255 shapes that inherit a style's formula", first, second);
    assert_eq!(first.commits, 1, "{first:?}");
    assert!(first.steps <= 1_200 * u64::from(PAGES), "a few hundred steps for each cell: {first:?}");
    assert!(second.steps <= first.steps, "the listing is kept: {second:?} against {first:?}");
    assert_eq!(value(&mut s, PAGES, 10, "LineWeight"), json!(16.0));
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_reaches_255_shapes_that_inherit_a_style_formula_in_one_commit() {
    a_write_reaches_255_shapes_that_inherit_a_style_formula();
}

timed_twin!(a_write_reaches_255_shapes_that_inherit_a_style_formula_is_timed, 30, a_write_reaches_255_shapes_that_inherit_a_style_formula);

/// 256 pages whose formulas name no page, no sheet no page holds and whose styles hold none: **a write costs what it cost before the listing knew the sheets no page holds**, a step or two for each
/// page (the listing is a look at the text of each formula) and nothing for the style's links.
fn a_write_to_256_pages_that_name_nothing_costs_the_listing_once() {
    let plain = StyleBuilder::new().name("Plain").cell("LineWeight", "3");
    let mut b = DocBuilder::new().style(1, plain);
    for p in 1..=PAGES {
        let shape = ShapeBuilder::new(10).links(Some(1), None, None).cell("Width", "1").inh("LineWeight", Some("3"));
        b = b.page(p, PageBuilder::new().shape(shape).shape(ShapeBuilder::new(11).formula("Height", "Sheet.10!Width*2", "2")));
    }
    let mut s = session_of(b.build());
    let (_, first) = counted(&mut s, "sheet.set_cell", json!({"page": 5, "id": 10, "cell": "Width", "formula": "4"}));
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 6, "id": 10, "cell": "Width", "formula": "4"}));
    the_costs("256 pages that name nothing", first, second);
    assert_eq!(first.commits, 1, "{first:?}");
    // 285 and 33 steps before the listing knew the sheets no page holds, and a step for each of the document's two styles more now.
    assert!(first.steps <= 4 * u64::from(PAGES), "the listing is a step or two for each page: {first:?}");
    assert!(second.steps <= 100, "the listing is kept and nothing reads a style: {second:?}");
    assert_eq!(value(&mut s, 5, 11, "Height"), json!(8.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_to_256_pages_that_name_nothing_costs_nothing_for_the_sheets_no_page_holds() {
    a_write_to_256_pages_that_name_nothing_costs_the_listing_once();
}

timed_twin!(a_write_to_256_pages_that_name_nothing_is_timed, 30, a_write_to_256_pages_that_name_nothing_costs_the_listing_once);
