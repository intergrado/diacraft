//! A write that cells of other pages read (gap F40): the cells of the other pages that name a cell of the page through `Pages[..]` are worked out in the same call, so that the
//! drawing is what a full evaluation gives, and **undo and redo put every page back exactly**, and **a save and a reopen give the pages the values the session has** in both formats that hold
//! formulas. The counted costs (64 pages of 2,000 shapes, a chain through 200 pages, a chain that passes the budget) are `sheet_hostile`'s.

use diacraft_calc::{Budget, recalc_all};
use diacraft_doc::Document;
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::native_common::Dir;
use crate::{blank_session, check_session, run, snapshot};

pub(crate) fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// `pages` pages, `Page-1` to `Page-<pages>`: the width of the shape 10 of the first is 1, and that of each later page is that of the page before it, plus 1. The second shape of each page
/// reads the width of the first on its own page, so that a page that is reached passes the change on inside itself.
fn chain(pages: u32) -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=pages {
        let first = if p == 1 {
            ShapeBuilder::new(10).cell("Width", "1")
        } else {
            ShapeBuilder::new(10).formula("Width", &format!("Pages[Page-{}]!Sheet.10!Width+1", p - 1), &p.to_string())
        };
        let follows = ShapeBuilder::new(11).formula("Height", "Sheet.10!Width*2", &(2 * p).to_string());
        b = b.page(p, PageBuilder::new().name(&format!("Page-{p}")).shape(first).shape(follows));
    }
    session_of(b.build())
}

fn cell(s: &mut Session, page: u32, id: u32, name: &str) -> Value {
    run(s, "sheet.cell", json!({"page": page, "id": id, "cell": name}))["value"].clone()
}

/// The widths of shape 10 and the heights of shape 11 of each page.
fn read(s: &mut Session, pages: u32) -> Vec<(Value, Value)> {
    (1..=pages).map(|p| (cell(s, p, 10, "Width"), cell(s, p, 11, "Height"))).collect()
}

fn the_values(first: f64, pages: u32) -> Vec<(Value, Value)> {
    (0..pages).map(|k| (json!(first + f64::from(k)), json!(2.0 * (first + f64::from(k))))).collect()
}

/// No cell of the drawing differs from what a full evaluation gives.
pub(crate) fn settled(doc: &Document) {
    let report = recalc_all(doc, &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "a full evaluation differs in {:?}", report.differed);
}

/// The write is one step of the history, whatever the pages it reached; undo puts every page back, redo works them out again, and each is the drawing exactly (the document compared whole).
#[test]
fn undo_and_redo_of_a_write_that_other_pages_read_put_every_page_back_exactly() {
    let mut s = chain(6);
    let (start, steps) = (snapshot(&s), snapshot(&s).undo_steps);
    run(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    let written = snapshot(&s);
    assert_eq!(written.undo_steps, steps + 1, "one step for the write and the pages it reached");
    assert_eq!(read(&mut s, 6), the_values(5.0, 6));
    settled(s.document().unwrap());

    run(&mut s, "edit.undo", json!({}));
    assert!(snapshot(&s).document == start.document, "undo puts every page back, the cells that were worked out too");
    assert_eq!(read(&mut s, 6), the_values(1.0, 6));
    settled(s.document().unwrap());

    run(&mut s, "edit.redo", json!({}));
    assert!(snapshot(&s).document == written.document, "redo gives the drawing as the write left it");
    assert_eq!(read(&mut s, 6), the_values(5.0, 6));
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

/// Undo of a write after another write that reached the same pages, and a write in the middle of the chain, which reaches the pages after it and not before.
#[test]
fn a_write_in_the_middle_of_a_chain_reaches_the_pages_after_it_and_undo_takes_each_back() {
    let mut s = chain(6);
    run(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    let first = snapshot(&s).document;
    run(&mut s, "sheet.set_cell", json!({"page": 4, "id": 10, "cell": "Width", "formula": "100"}));
    assert_eq!(read(&mut s, 6)[..3], the_values(5.0, 3)[..], "the pages before it are as they were");
    assert_eq!(cell(&mut s, 5, 10, "Width"), json!(101.0));
    assert_eq!(cell(&mut s, 6, 10, "Width"), json!(102.0));
    settled(s.document().unwrap());
    run(&mut s, "edit.undo", json!({}));
    assert!(snapshot(&s).document == first);
    assert_eq!(read(&mut s, 6), the_values(5.0, 6));
    settled(s.document().unwrap());
}

/// A saved file gives the pages the values the session has: the cached values the write left on the later pages are saved as they are, and a drawing read from the file is settled and goes on
/// following a write (the listing of the sheets that name a page is made again in the new session).
#[test]
fn a_save_and_a_reopen_give_the_pages_the_values_the_session_has_in_each_format() {
    let dir = Dir::new();
    for ext in ["diacraft", "vsdx"] {
        let mut s = chain(6);
        run(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
        let path = dir.path(&format!("cross-pages.{ext}"));
        run(&mut s, "file.save_as", json!({"path": path}));
        let mut reopened = Session::new();
        reopened.execute("file.open", &json!({"path": path})).unwrap_or_else(|e| panic!("the {ext} file does not open: {e}"));
        assert_eq!(read(&mut reopened, 6), the_values(5.0, 6), "{ext}");
        settled(reopened.document().unwrap());
        if ext == "diacraft" {
            assert!(s.document() == reopened.document(), "the drawing read back from a .diacraft file is the drawing that was saved");
        }
        run(&mut reopened, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "40"}));
        assert_eq!(read(&mut reopened, 6), the_values(40.0, 6), "{ext}: a write in the session that read the file reaches the pages");
        settled(reopened.document().unwrap());
    }
}

/// The other commands that edit a page reach the pages that read it too: a resize of the shape.
#[test]
fn a_command_that_resizes_a_shape_works_out_the_cells_of_other_pages_that_read_it() {
    let mut s = chain(4);
    run(&mut s, "shape.resize", json!({"id": 10, "x": 3.0, "y": 2.0, "width": 3.0, "height": 1.5}));
    assert_eq!(cell(&mut s, 1, 10, "Width"), json!(3.0));
    assert_eq!(cell(&mut s, 2, 10, "Width"), json!(4.0));
    assert_eq!(cell(&mut s, 4, 10, "Width"), json!(6.0));
    assert_eq!(cell(&mut s, 4, 11, "Height"), json!(12.0), "the cell of the page that is reached, which reads the one that changed");
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

/// A shape of another page that is moved by a cell that reads this page is a shape the call moved: the call lists the cell, and the drawing is settled.
#[test]
fn a_pin_that_reads_another_page_follows_it_and_is_listed_with_the_cells_the_call_worked_out() {
    let mut b = DocBuilder::new();
    b = b.page(1, PageBuilder::new().name("One").shape(ShapeBuilder::new(10).cell("PinX", "1").cell("Width", "1")));
    b = b.page(2, PageBuilder::new().name("Two").shape(ShapeBuilder::new(10).formula("PinX", "Pages[One]!Sheet.10!PinX+2", "3").cell("Width", "1")));
    let mut s = session_of(b.build());
    let answer = run(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "PinX", "formula": "4"}));
    assert_eq!(cell(&mut s, 2, 10, "PinX"), json!(6.0));
    let r = &answer["recalculated"];
    assert_eq!(r["cells"], json!(1), "{r}");
    assert_eq!(r["listed"][0]["sheet"], json!({"kind": "shape", "page": 2, "id": 10}), "the cell is on page 2: {r}");
    assert_eq!(r["listed"][0]["cell"], json!("PinX"), "{r}");
    settled(s.document().unwrap());
    assert_eq!(check_session(&s), Ok(()));
}

// ---- the connectors of a page that is only reached ----------------------------------------------------------------------------------------

/// A drawing of two pages, "One" and "Two": a rectangle on page 1, and on page 2 two rectangles and a connector glued from the first to the second. With `reads` the first rectangle of page 2 has
/// its `PinX` read the rectangle of page 1 through `Pages[One]!` and add 1 (it is where it was dropped, 2).
fn two_pages_with_a_connector(reads: bool) -> (Session, u32, u32, u32) {
    let id_of = |v: &Value| u32::try_from(v["id"].as_u64().unwrap()).unwrap();
    let mut s = blank_session();
    run(&mut s, "page.rename", json!({"id": 1, "name": "One"}));
    let two = u32::try_from(run(&mut s, "page.add", json!({"name": "Two"}))["id"].as_u64().unwrap()).unwrap();
    let one = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": 1, "x": 1.0, "y": 1.0})));
    let a = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 2.0, "y": 2.0})));
    let b = id_of(&run(&mut s, "shape.drop", json!({"master": "rectangle", "page": two, "x": 6.0, "y": 4.0})));
    let c = id_of(&run(&mut s, "connector.add", json!({"page": two, "from": {"id": a, "point": 3}, "to": {"id": b, "point": 1}})));
    if reads {
        let formula = format!("Pages[One]!Sheet.{one}!PinX+1");
        run(&mut s, "sheet.set_cell", json!({"page": two, "id": a, "cell": "PinX", "formula": formula}));
    }
    (s, one, a, c)
}

/// The sheet of shape `id` of page `page`, as the document holds it.
pub(crate) fn sheet_of(s: &Session, page: u32, id: u32) -> std::sync::Arc<diacraft_doc::Sheet> {
    let doc = s.document().unwrap();
    let shapes = &doc.page(page).unwrap().shapes;
    std::sync::Arc::clone(&shapes.iter().find(|shape| shape.id == id).unwrap().sheet)
}

/// A rectangle of page 2 that is moved by a write on page 1 has the connector glued to it follow, routed as a direct write that moves it routes it: the connector's cells are the same as in a
/// drawing where the rectangle was moved by writing its own `PinX`.
#[test]
fn a_connector_glued_to_a_shape_that_another_page_moved_is_as_a_direct_move_leaves_it() {
    let (mut through, one, a, c) = two_pages_with_a_connector(true);
    let (mut direct, _, _, _) = two_pages_with_a_connector(false);
    let two = 2;
    assert!(sheet_of(&through, two, c) == sheet_of(&direct, two, c), "the two drawings start alike");
    run(&mut through, "sheet.set_cell", json!({"page": 1, "id": one, "cell": "PinX", "formula": "4"}));
    run(&mut direct, "sheet.set_cell", json!({"page": two, "id": a, "cell": "PinX", "formula": "5"}));
    assert_eq!(cell(&mut through, two, a, "PinX"), json!(5.0));
    assert!(sheet_of(&through, two, c) == sheet_of(&direct, two, c), "the connector is where a direct move puts it, route and ends");
    settled(through.document().unwrap());
    assert_eq!(check_session(&through), Ok(()));
}

/// A write to a master works the pages that use it out whole, and a page that uses no instance of it but reads one that does is worked out after them (one more commit for the pages that
/// name the page): the document is what a full evaluation gives.
#[test]
fn a_write_to_a_master_reaches_a_page_that_reads_a_page_that_uses_it() {
    let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", "1"));
    let doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("One").shape(ShapeBuilder::new(10).master(1)))
        .page(2, PageBuilder::new().name("Two").shape(ShapeBuilder::new(10).formula("Width", "Pages[One]!Sheet.10!Width*10", "10")))
        .page(3, PageBuilder::new().name("Three").shape(ShapeBuilder::new(10).formula("Width", "Pages[Two]!Sheet.10!Width+1", "11")))
        .build();
    let mut s = session_of(doc);
    run(&mut s, "sheet.set_cell", json!({"master": 1, "id": 5, "cell": "Width", "formula": "3"}));
    assert_eq!(cell(&mut s, 1, 10, "Width"), json!(3.0), "the instance reads the master's cell");
    assert_eq!(cell(&mut s, 2, 10, "Width"), json!(30.0), "the page that uses no instance of the master and reads the page that does");
    assert_eq!(cell(&mut s, 3, 10, "Width"), json!(31.0), "and the page that reads that");
    settled(s.document().unwrap());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((cell(&mut s, 2, 10, "Width"), cell(&mut s, 3, 10, "Width")), (json!(10.0), json!(11.0)), "undo puts the pages back");
    settled(s.document().unwrap());
}
