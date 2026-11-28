//! The cells that read other pages (the rest of gap F40): **a command that changes the pages themselves** (a page renamed, added, copied, deleted or moved) works out the cells whose
//! formulas name a page; **a formula a master, a style or the document's sheet holds** is listed with the others, and an instance that inherits one follows it; **a formula that reads at run time**
//! (`EVALTEXT`) is worked out after a change to any other page; and **the connectors glued to a shape that another page moved are routed again** whatever command made the change.
//!
//! Each is held to what no change may break: a full evaluation changes no cell after the command, undo and redo give the drawing back exactly (the document compared whole), and a save and a
//! reopen in both formats give the pages the values the session has. The costs on 256 pages are **counts** (the steps the evaluator took and the commits it made), with a timed twin that is
//! `#[ignore]`d.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_calc::work;
use diacraft_doc::{Cell, CellAddr, RowKey, Value as Held};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};
use serde_json::{Value, json};

use crate::native_common::Dir;
use crate::sheet_cross_pages::{session_of, settled, sheet_of};
use crate::{blank_session, check_session, run, snapshot};

/// The whole answer of `sheet.cell` for a cell of a shape of a page.
pub(crate) fn full(s: &mut Session, page: u32, id: u32, name: &str) -> Value {
    run(s, "sheet.cell", json!({"page": page, "id": id, "cell": name}))
}

pub(crate) fn value(s: &mut Session, page: u32, id: u32, name: &str) -> Value {
    full(s, page, id, name)["value"].clone()
}

pub(crate) fn error(s: &mut Session, page: u32, id: u32, name: &str) -> Value {
    full(s, page, id, name)["error"].clone()
}

/// A formula cell with the error a formula that finds no page saved: its last good value and `#REF!`.
fn finds_nothing(formula: &str) -> Cell {
    let mut cell = Cell::with_formula(formula, Some(Held::from_text("0")));
    cell.error = Some(Arc::from("#REF!"));
    cell
}

/// Runs `command` and holds the drawing to what no change may break (see the module documentation); `expect` says what the pages hold after it, and is asked again after a redo and of the
/// drawing read back from each file.
pub(crate) fn exact(s: &mut Session, command: &str, params: Value, expect: impl Fn(&mut Session)) {
    settled(s.document().unwrap());
    let start = snapshot(s);
    run(s, command, params);
    let done = snapshot(s);
    assert_eq!(done.undo_steps, start.undo_steps + 1, "{command} is one step");
    expect(s);
    settled(s.document().unwrap());
    run(s, "edit.undo", json!({}));
    assert!(snapshot(s).document == start.document, "undo gives the drawing back exactly, the cells that were worked out too");
    run(s, "edit.redo", json!({}));
    assert!(snapshot(s).document == done.document, "redo gives the drawing as the command left it");
    expect(s);
    settled(s.document().unwrap());
    let dir = Dir::new();
    for ext in ["diacraft", "vsdx"] {
        let path = dir.path(&format!("page-readers.{ext}"));
        run(s, "file.save_as", json!({"path": path}));
        let mut reopened = Session::new();
        reopened.execute("file.open", &json!({"path": path})).unwrap_or_else(|e| panic!("the {ext} file does not open: {e}"));
        expect(&mut reopened);
        settled(reopened.document().unwrap());
        if ext == "diacraft" {
            assert!(s.document() == reopened.document(), "the drawing read back from a .diacraft file is the drawing that was saved");
        }
    }
    assert_eq!(check_session(s), Ok(()));
}

// ---- a page renamed, added, copied, deleted, moved -----------------------------------------------------------------------------------------

/// Page 1 is "Plan" by its `Name` and "Site-1" by its `NameU`, and holds a shape 3 of width 5. Page 2 reads that width by the name "Plan" (shape 10), and shape 11 reads shape 10.
fn plan_and_reader() -> Session {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(
            2,
            PageBuilder::new()
                .name("Reader")
                .shape(ShapeBuilder::new(10).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"))
                .shape(ShapeBuilder::new(11).formula("Height", "Sheet.10!Width+1", "11")),
        )
        .build();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Site-1"));
    session_of(doc)
}

/// A page renamed is a name that no formula finds: the cell that read it by that name reads `#REF!`, with the last good value, which what reads it on its page goes on reading (a cell read
/// through a reference gives its last good value and not its error), and a rename back gives the cell its value with no error.
#[test]
fn renaming_a_page_works_out_the_cells_that_name_it() {
    let mut s = plan_and_reader();
    exact(&mut s, "page.rename", json!({"id": 1, "name": "Dach"}), |s| {
        assert_eq!(error(s, 2, 10, "Width"), json!("#REF!"), "the page the formula names is not there");
        assert_eq!(value(s, 2, 10, "Width"), json!(10.0), "its last good value stands");
        assert_eq!((value(s, 2, 11, "Height"), error(s, 2, 11, "Height")), (json!(11.0), Value::Null), "and what reads it reads that");
    });
    exact(&mut s, "page.rename", json!({"id": 1, "name": "Plan"}), |s| {
        assert_eq!((value(s, 2, 10, "Width"), error(s, 2, 10, "Width")), (json!(10.0), Value::Null), "the name finds the page again");
    });
}

/// A page that is added is a name that finds a page: the cell that read a page that was not there reads it.
#[test]
fn adding_a_page_works_out_the_cells_that_name_it() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("One").shape(ShapeBuilder::new(10).set(CellAddr::direct("Width"), finds_nothing("Pages[Plan]!PageWidth*2"))))
        .page(2, PageBuilder::new().name("Two"))
        .build();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Site-1"));
    let mut s = session_of(doc);
    exact(&mut s, "page.add", json!({"name": "Plan"}), |s| {
        assert_eq!((value(s, 1, 10, "Width"), error(s, 1, 10, "Width")), (json!(22.0), Value::Null), "the new page's width is 11 inches");
    });
}

/// A page that is copied is a page with a name of its own: a formula that named the copy before it was there reads it.
#[test]
fn copying_a_page_works_out_the_cells_that_name_the_copy() {
    let reader = ShapeBuilder::new(10).set(CellAddr::direct("Width"), finds_nothing("Pages[One-2]!Sheet.3!Width+1"));
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("One").shape(ShapeBuilder::new(3).cell("Width", "7")))
        .page(2, PageBuilder::new().name("Two").shape(reader))
        .build();
    let mut s = session_of(doc);
    exact(&mut s, "page.duplicate", json!({"id": 1}), |s| {
        assert_eq!((value(s, 2, 10, "Width"), error(s, 2, 10, "Width")), (json!(8.0), Value::Null), "the copy is called One-2 and holds the shape");
    });
}

/// A page that is deleted is a name that finds nothing.
#[test]
fn deleting_a_page_works_out_the_cells_that_name_it() {
    let mut s = plan_and_reader();
    exact(&mut s, "page.delete", json!({"id": 1}), |s| {
        assert_eq!(error(s, 2, 10, "Width"), json!("#REF!"));
    });
}

/// A move that renumbers renames the pages that have a default name, and the name a formula gives finds another page: the first page of the list that has the name is the one found.
#[test]
fn moving_a_page_with_renumbering_works_out_the_cells_that_name_a_page_by_a_number_it_took() {
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "3")))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "4")))
        .page(3, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Width", "Pages[Page-1]!Sheet.10!Width", "3")))
        .build();
    let mut s = session_of(doc);
    exact(&mut s, "page.move", json!({"id": 2, "to": 0, "renumber": true}), |s| {
        assert_eq!(value(s, 3, 10, "Width"), json!(4.0), "page 2 is the first of the list now, and is called Page-1");
    });
}

/// A formula that reads the list of pages (`PAGENAME()`, `PAGENUMBER()`, `PAGECOUNT()`) names no page, and is worked out after a change to the list.
#[test]
fn a_cell_that_reads_the_list_of_pages_follows_it() {
    let shape = ShapeBuilder::new(10)
        .row_formula("User", None, RowKey::name("Where"), "Value", "PAGENAME()", "Reader")
        .row_formula("User", None, RowKey::name("Place"), "Value", "PAGENUMBER()", "2")
        .row_formula("User", None, RowKey::name("Count"), "Value", "PAGECOUNT()", "2");
    let doc = DocBuilder::new().page(1, PageBuilder::new().name("One")).page(2, PageBuilder::new().name("Reader").shape(shape)).build();
    let mut s = session_of(doc);
    exact(&mut s, "page.rename", json!({"id": 2, "name": "Desk"}), |s| {
        assert_eq!(value(s, 2, 10, "User.Where.Value"), json!("Desk"));
    });
    exact(&mut s, "page.add", json!({"name": "Third"}), |s| {
        assert_eq!(value(s, 2, 10, "User.Count.Value"), json!(3.0));
    });
    exact(&mut s, "page.move", json!({"id": 2, "to": 0}), |s| {
        assert_eq!(value(s, 2, 10, "User.Place.Value"), json!(1.0));
    });
}

// ---- a formula a master, a style or the document's sheet holds -----------------------------------------------------------------------------

/// Master 1 holds a shape 5 whose `Width` reads shape 3 of the page "Plan"; page 2 holds an instance of it that inherits the formula.
fn master_that_reads_a_page() -> Session {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).master(1).inh("Width", Some("10"))))
        .build();
    session_of(doc)
}

/// A write to the cell a master's formula reads works out the master's own cell and the instance that inherits the formula, which is on a page that holds no formula of its own that names a page.
#[test]
fn a_write_works_out_the_master_and_the_instance_that_inherit_a_formula_that_names_a_page() {
    let mut s = master_that_reads_a_page();
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(value(s, 2, 10, "Width"), json!(14.0), "the instance reads the page through the formula it inherits");
        assert_eq!(run(s, "sheet.cell", json!({"master": 1, "id": 5, "cell": "Width"}))["value"], json!(14.0), "and so does the master's own cell");
    });
}

/// A page renamed is a name the inherited formula does not find either.
#[test]
fn renaming_a_page_works_out_the_master_and_the_instance_that_inherit_a_formula_that_names_it() {
    let mut s = master_that_reads_a_page();
    run(&mut s, "page.rename", json!({"id": 1, "name": "Dach"}));
    // The page's `NameU` is its `Name` here, and is not changed by a rename: the name still finds it. A page made with another `NameU` loses it.
    assert_eq!(error(&mut s, 2, 10, "Width"), Value::Null);
    let mut doc = s.document().unwrap().clone();
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name_u = Some(Arc::from("Site-1"));
    Arc::make_mut(doc.pages.get_mut(0).unwrap()).name = Arc::from("Plan");
    let mut s = session_of(doc);
    exact(&mut s, "page.rename", json!({"id": 1, "name": "Dach"}), |s| {
        assert_eq!(error(s, 2, 10, "Width"), json!("#REF!"), "the instance");
        assert_eq!(run(s, "sheet.cell", json!({"master": 1, "id": 5, "cell": "Width"}))["error"], json!("#REF!"), "and the master");
    });
}

/// A cell of the document's sheet, and one of a style, that name a page are worked out by a write to the cell they read.
#[test]
fn a_write_works_out_the_cells_of_the_document_and_of_a_style_that_name_a_page() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let mut doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Other"))
        .build();
    let total = CellAddr::in_row("User", None, RowKey::name("Total"), "Value");
    Arc::make_mut(&mut doc.sheet).set_cell(&total, Cell::with_formula("Pages[Plan]!Sheet.3!Width*3", Some(Held::from_text("15")))).unwrap();
    let mut s = session_of(doc);
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}), |s| {
        assert_eq!(run(s, "sheet.cell", json!({"document": true, "cell": "User.Total.Value"}))["value"], json!(21.0), "the document's cell");
        assert_eq!(run(s, "sheet.cell", json!({"style": 1, "cell": "LineWeight"}))["value"], json!(14.0), "the style's cell");
    });
}

// ---- a formula that reads at run time ----------------------------------------------------------------------------------------------------

/// A shape whose text names a cell of another page, and whose `Height` works the text out as a formula.
fn evaluates_text_that_names_a_page() -> Session {
    let reader = ShapeBuilder::new(10).text("Pages[Plan]!Sheet.3!Width").formula("Height", "EVALTEXT()", "5");
    let doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(reader))
        .build();
    session_of(doc)
}

/// What a formula that reads at run time (`EVALTEXT`) reads is not in its text, so a change to any cell of another page works it out.
#[test]
fn a_write_works_out_a_cell_that_reads_a_page_at_run_time() {
    let mut s = evaluates_text_that_names_a_page();
    exact(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "9"}), |s| {
        assert_eq!(value(s, 2, 10, "Height"), json!(9.0), "the text names a page, and the cell reads it");
    });
}

// ---- the connectors glued to a shape that another page moved -----------------------------------------------------------------------------------

/// Page "One" holds a rectangle, and page "Two" two rectangles with a connector glued from the first to the second. The first of page 2 has its `PinX` read the rectangle of page 1
/// through `Pages[One]!`, by the formula `formula(id of the rectangle of page 1)`, and is where it was dropped (2). Without a formula the drawing is the twin in which a write moves it.
fn two_pages_with_a_connector(formula: Option<&dyn Fn(u32) -> String>) -> (Session, u32, u32, u32) {
    let id_of = |v: &Value| u32::try_from(v["id"].as_u64().unwrap()).unwrap();
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let two = u32::try_from(run(&mut s, "page.add", json!({"name": "Two"}))["id"].as_u64().unwrap()).unwrap();
    let one = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"page": two, "from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    // A data field of the rectangle of page 1, which a formula may read.
    run(&mut s, "shapedata.define", json!({"id": one, "page": 1, "fields": [{"name": "Offset", "label": "Offset", "type": "number", "value": 1}]}));
    if let Some(formula) = formula {
        run(&mut s, "sheet.set_cell", json!({"page": two, "id": a, "cell": "PinX", "formula": formula(one)}));
    }
    (s, one, a, c)
}

/// Runs `command` on the drawing in which page 2 reads page 1 and the write that gives the same pin on the drawing that holds no formula, and holds the connector of page 2 to be where
/// the write puts it, route and ends.
fn the_connector_is_where_a_direct_move_leaves_it(formula: &dyn Fn(u32) -> String, command: &str, params: &dyn Fn(u32) -> Value) {
    let (mut through, one, a, c) = two_pages_with_a_connector(Some(formula));
    let (mut direct, _, _, _) = two_pages_with_a_connector(None);
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the two drawings start alike");
    run(&mut through, command, params(one));
    run(&mut direct, "sheet.set_cell", json!({"page": 2, "id": a, "cell": "PinX", "formula": "5"}));
    assert_eq!(value(&mut through, 2, a, "PinX"), json!(5.0), "{command}: the rectangle of page 2 follows");
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "{command}: the connector is where a direct move puts it, route and ends");
    settled(through.document().unwrap());
    assert_eq!(check_session(&through), Ok(()));
}

fn pin_x_plus_one(id: u32) -> String {
    format!("Pages[One]!Sheet.{id}!PinX+1")
}

#[test]
fn a_move_of_a_shape_routes_the_connectors_of_the_page_that_reads_it() {
    the_connector_is_where_a_direct_move_leaves_it(&pin_x_plus_one, "shape.move", &|one| json!({"ids": [one], "page": 1, "dx": 3.0, "dy": 0.0}));
}

#[test]
fn a_resize_of_a_shape_routes_the_connectors_of_the_page_that_reads_it() {
    the_connector_is_where_a_direct_move_leaves_it(
        &pin_x_plus_one,
        "shape.resize",
        &|one| json!({"id": one, "page": 1, "x": 3.5, "y": 0.5, "width": 1.0, "height": 1.0}),
    );
}

#[test]
fn an_edit_of_shape_data_routes_the_connectors_of_the_page_that_reads_it() {
    the_connector_is_where_a_direct_move_leaves_it(
        &|id| format!("Pages[One]!Sheet.{id}!Prop.Offset.Value+1"),
        "shapedata.set",
        &|one| json!({"id": one, "page": 1, "row": "Offset", "value": 4}),
    );
}

/// A page command that moves a shape of another page (a copy of a page that a formula named before it was there) routes the connectors glued to it, as a write that moves it does.
#[test]
fn a_page_command_that_moves_a_shape_of_another_page_routes_its_connectors() {
    let (mut through, one, a, c) = two_pages_with_a_connector(Some(&|id| format!("Pages[One-2]!Sheet.{id}!PinX+1")));
    let (mut direct, _, _, _) = two_pages_with_a_connector(None);
    assert_eq!(error(&mut through, 2, a, "PinX"), json!("#REF!"), "the page One-2 is not there yet");
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the two drawings start alike");
    run(&mut through, "shape.move", json!({"ids": [one], "page": 1, "dx": 3.0, "dy": 0.0}));
    // The copy holds the rectangle at x = 4: the formula reads it, and adds 1.
    run(&mut through, "page.duplicate", json!({"id": 1}));
    run(&mut direct, "sheet.set_cell", json!({"page": 2, "id": a, "cell": "PinX", "formula": "5"}));
    assert_eq!((value(&mut through, 2, a, "PinX"), error(&mut through, 2, a, "PinX")), (json!(5.0), Value::Null));
    assert!(sheet_of(&through, 2, c) == sheet_of(&direct, 2, c), "the connector is where a direct move puts it, route and ends");
    settled(through.document().unwrap());
    assert_eq!(check_session(&through), Ok(()));
}

// ---- 256 pages, counted ------------------------------------------------------------------------------------------------------------------------

pub(crate) const PAGES: u32 = 256;

/// What a call cost, as counts: the steps the evaluator took and the commits it made.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cost {
    pub(crate) steps: u64,
    pub(crate) commits: u64,
}

pub(crate) fn counted(s: &mut Session, id: &str, params: Value) -> (Value, Cost) {
    work::reset();
    let answer = run(s, id, params);
    (answer, Cost { steps: work::steps(), commits: work::commits() })
}

/// A timed twin of a counted case: `#[ignore]`d, and run by hand on a quiet machine (`--ignored --nocapture`).
macro_rules! timed_twin {
    ($name:ident, $seconds:expr, $case:ident) => {
        #[test]
        #[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
        fn $name() {
            let started = Instant::now();
            $case();
            let took = started.elapsed();
            eprintln!("took {took:?}");
            assert!(took < Duration::from_secs($seconds), "took {took:?}, more than {} s", $seconds);
        }
    };
}
pub(crate) use timed_twin;

/// 256 pages called "Pg1" to "Pg256" by `Name` and "N1" to "N256" by `NameU`: shape 10 of each page after the first reads that of the page before it **by its name**. Page 257, "Decoy" by
/// its `NameU` and "Pg99" by its `Name`, comes after the others and holds a width of 1000: the page the name Pg99 finds once page 99 is renamed.
fn named_chain() -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=PAGES {
        let first = if p == 1 {
            ShapeBuilder::new(10).cell("Width", "1")
        } else {
            ShapeBuilder::new(10).formula("Width", &format!("Pages[Pg{}]!Sheet.10!Width+1", p - 1), &p.to_string())
        };
        b = b.page(p, PageBuilder::new().name(&format!("Pg{p}")).shape(first));
    }
    b = b.page(PAGES + 1, PageBuilder::new().name("Pg99").shape(ShapeBuilder::new(10).cell("Width", "1000")));
    let mut doc = b.build();
    for p in 0..PAGES as usize {
        Arc::make_mut(doc.pages.get_mut(p).unwrap()).name_u = Some(Arc::from(format!("N{}", p + 1)));
    }
    Arc::make_mut(doc.pages.get_mut(PAGES as usize).unwrap()).name_u = Some(Arc::from("Decoy"));
    session_of(doc)
}

/// A page renamed in a chain of 256 pages that read each other by name: the cell that names it finds the page that has the name now, and the 156 pages after it follow, in **one commit**
/// and steps that are the cells' (a few hundred each), and the name given back works them all out again in one more.
fn renaming_a_page_of_a_chain_of_256_pages() {
    let mut s = named_chain();
    let (_, cost) = counted(&mut s, "page.rename", json!({"id": 99, "name": "X99"}));
    eprintln!("a chain of {PAGES} pages, a page renamed: {cost:?}");
    assert_eq!(cost.commits, 1, "one commit for the 157 cells: {cost:?}");
    assert!(cost.steps <= 400 * u64::from(PAGES), "a few hundred steps for each cell and no more: {cost:?}");
    assert_eq!(value(&mut s, 100, 10, "Width"), json!(1001.0), "the decoy is the page called Pg99 now");
    assert_eq!(value(&mut s, PAGES, 10, "Width"), json!(1001.0 + f64::from(PAGES - 100)), "the pages after it follow");
    assert_eq!(value(&mut s, 99, 10, "Width"), json!(99.0), "the pages before it are as they were");
    settled(s.document().unwrap());
    // The decoy, called Pg99 as a file may call it, is given another name first: a command gives no page the name another has, and the name can be given back then.
    run(&mut s, "page.rename", json!({"id": PAGES + 1, "name": "Decoy"}));
    let (_, back) = counted(&mut s, "page.rename", json!({"id": 99, "name": "Pg99"}));
    assert_eq!(back.commits, 1, "{back:?}");
    assert!(back.steps <= 400 * u64::from(PAGES), "{back:?}");
    assert_eq!((value(&mut s, PAGES, 10, "Width"), error(&mut s, PAGES, 10, "Width")), (json!(f64::from(PAGES)), Value::Null));
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn renaming_a_page_of_a_chain_of_256_pages_is_one_commit() {
    renaming_a_page_of_a_chain_of_256_pages();
}

timed_twin!(renaming_a_page_of_a_chain_of_256_pages_is_timed, 30, renaming_a_page_of_a_chain_of_256_pages);

/// 256 pages whose formulas name no page: the first change to the list is the listing of the pages (a step for 64 bytes of each formula), kept, and the next costs next to nothing: no
/// cell is read by a name, so none is worked out, and no graph of a page is made for it.
fn renaming_a_page_of_256_pages_that_name_no_page() {
    let mut b = DocBuilder::new();
    for p in 1..=PAGES {
        b = b.page(
            p,
            PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")).shape(ShapeBuilder::new(11).formula(
                "Height",
                "Sheet.10!Width*2",
                "2",
            )),
        );
    }
    let mut s = session_of(b.build());
    let (_, first) = counted(&mut s, "page.rename", json!({"id": 5, "name": "Five"}));
    let (_, second) = counted(&mut s, "page.rename", json!({"id": 6, "name": "Six"}));
    let (_, third) = counted(&mut s, "page.add", json!({"name": "Late"}));
    eprintln!("{PAGES} pages that name no page: the first {first:?}, the second {second:?}, a page added {third:?}");
    assert!(first.steps <= 4 * u64::from(PAGES), "the listing is a step or two for each page: {first:?}");
    assert!(second.steps <= 100 && third.steps <= 100, "the listing is kept and nothing reads a name: {second:?} {third:?}");
    assert_eq!(second.commits + third.commits, 0, "nothing is worked out");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn renaming_a_page_of_256_pages_that_name_no_page_costs_the_listing_once() {
    renaming_a_page_of_256_pages_that_name_no_page();
}

timed_twin!(renaming_a_page_of_256_pages_that_name_no_page_is_timed, 30, renaming_a_page_of_256_pages_that_name_no_page);

/// 256 pages, each holding an instance of a master whose width reads the page "Plan": a write to the cell it reads works out the master's cell and the 255 instances in **one commit**
/// and steps that are theirs.
fn a_write_reaches_255_instances_of_a_master_that_names_a_page() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    let mut b = DocBuilder::new().master(1, master);
    b = b.page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")));
    for p in 2..=PAGES {
        b = b.page(p, PageBuilder::new().shape(ShapeBuilder::new(10).master(1).inh("Width", Some("10"))));
    }
    let mut s = session_of(b.build());
    let (answer, cost) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "7"}));
    eprintln!("{} instances of a master that names a page: {cost:?}", PAGES - 1);
    assert_eq!(cost.commits, 1, "{cost:?}");
    assert!(cost.steps <= 600 * u64::from(PAGES), "a few hundred steps for each cell: {cost:?}");
    assert!(answer["recalculated"]["cells"].as_u64().unwrap() >= u64::from(PAGES - 1), "{answer}");
    assert_eq!(value(&mut s, PAGES, 10, "Width"), json!(14.0));
    settled(s.document().unwrap());
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 3, "cell": "Width", "formula": "8"}));
    assert!(second.steps <= cost.steps, "the listing is kept: {second:?} against {cost:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_reaches_255_instances_of_a_master_that_names_a_page_in_one_commit() {
    a_write_reaches_255_instances_of_a_master_that_names_a_page();
}

timed_twin!(a_write_reaches_255_instances_of_a_master_that_names_a_page_is_timed, 30, a_write_reaches_255_instances_of_a_master_that_names_a_page);

/// 256 pages that each work a text out as a formula (`EVALTEXT`): a write to one page works the other 255 out in one commit and a few steps each; and 256 pages whose cells read
/// the members of a list or a container (which are cells of their own page) are not worked out by a write to another: **the rule that makes a sheet read every page is for the formulas
/// that can name a page at run time, and no others**.
fn a_write_reaches_255_pages_that_read_at_run_time() {
    let mut b = DocBuilder::new();
    for p in 1..=PAGES {
        let shape = ShapeBuilder::new(10).cell("Width", "1").text("Pages[P1]!Sheet.10!Width").formula("Height", "EVALTEXT()", "1");
        b = b.page(p, PageBuilder::new().name(&format!("P{p}")).shape(shape));
    }
    let mut s = session_of(b.build());
    let (_, cost) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "4"}));
    eprintln!("{PAGES} pages that read at run time: {cost:?}");
    assert_eq!(cost.commits, 1, "{cost:?}");
    assert!(cost.steps <= 800 * u64::from(PAGES), "{cost:?}");
    assert_eq!(value(&mut s, PAGES, 10, "Height"), json!(4.0), "the last page reads the first");
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_reaches_255_pages_that_read_at_run_time_in_one_commit() {
    a_write_reaches_255_pages_that_read_at_run_time();
}

timed_twin!(a_write_reaches_255_pages_that_read_at_run_time_is_timed, 30, a_write_reaches_255_pages_that_read_at_run_time);
