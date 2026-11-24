//! The fix round of the full review. Each test names the review's finding (M2, M4, M5, L6 to L9, mutants) and the spec rule or vector it pins;
//! each fails on the commit the review was made of (b8b136b), and passes now. The settings are the spec's "Settings for vectors" (see `snap_adoption.rs`).

use diacraft_doc::{Document, RowKey};
use diacraft_engine::Session;
use diacraft_engine::draw::{Turn, drop_shape, rotate_shapes};
use diacraft_engine::snap::{Query, Rect, SnapPage, SnapSettings, Snapped, Subject, Use, box_of, snap};
use diacraft_engine::stencil::find_master;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};
use serde_json::json;

use crate::{blank_session, run};

/// A page of rectangles, each `(id, x0, y0, x1, y1)` in paper inches.
pub fn page_of(shapes: &[(u32, f64, f64, f64, f64)]) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for (id, x0, y0, x1, y1) in shapes {
        let (w, h) = (x1 - x0, y1 - y0);
        page = page.shape(
            ShapeBuilder::new(*id)
                .cell("PinX", &((x0 + x1) / 2.0).to_string())
                .cell("PinY", &((y0 + y1) / 2.0).to_string())
                .cell("Width", &w.to_string())
                .cell("Height", &h.to_string())
                .cell("LocPinX", &(w / 2.0).to_string())
                .cell("LocPinY", &(h / 2.0).to_string())
                .cell("Angle", "0"),
        );
    }
    DocBuilder::new().page(1, page).build()
}

pub fn vector_settings() -> SnapSettings {
    let mut s = SnapSettings::none();
    s.snap = true;
    s.glue = true;
    s.guides = true;
    s.glue_guides = true;
    s.dynamic_grid = true;
    s.snap_connection_points = true;
    s.snap_vertices = true;
    s.snap_geometry = true;
    s.intersections = true;
    s
}

pub fn ask(doc: &Document, s: &SnapSettings, r: Rect, own: &[(f64, f64)], zoom: f64) -> Snapped {
    let page = SnapPage::read(doc, 1).unwrap();
    snap(doc, &page, s, &Query { subject: Subject::Box(r), using: Use::Place, moving: &[], zoom, own_points: own })
}

// ---------------------------------------------------------------------------------------------------------------- M2, the call's budgets are for its distinct shapes

/// `n` plain shapes round a 1 inch box, in the reach of a pull at 96 points to an inch and at 5.5.
pub fn shapes_round_a_box(n: u32) -> (Document, Rect) {
    let shapes: Vec<(u32, f64, f64, f64, f64)> = (0..n)
        .map(|i| {
            let (x, y) = (8.0 + f64::from(i % 5) * 0.3, 8.0 + f64::from(i / 5) * 0.3);
            (i + 1, x, y, x + 0.2, y + 0.2)
        })
        .collect();
    (page_of(&shapes), Rect { x0: 8.2, y0: 8.2, x1: 9.2, y1: 9.2 })
}

fn twelve_round_a_box() -> (Document, Rect) {
    shapes_round_a_box(12)
}

#[test]
fn m2_twelve_shapes_and_three_own_points_are_no_cut_of_the_outline_budget_which_is_sixteen_shapes_a_call() {
    // The review's case (12 shapes, 3 own points, zoom 5.5): the box's own points make the call ask the library twice (the outlines apart), and each query counted the
    // same twelve shapes against the call's sixteen, so the call said `truncated` and lost the outline snaps; the old function read the twelve once.
    let (doc, r) = twelve_round_a_box();
    for zoom in [96.0, 5.5] {
        for own in [vec![], vec![(8.5, 8.5), (8.7, 8.7), (9.0, 9.0)]] {
            let out = ask(&doc, &vector_settings(), r, &own, zoom);
            assert!(!out.truncated, "{} own points, zoom {zoom}: {out:?}", own.len());
        }
    }
}

#[test]
fn m2_the_most_shapes_a_call_reads_the_outlines_of_is_sixteen_with_places_of_its_own_as_without() {
    // Sixteen shapes in reach are all read, with and without own points; the seventeenth is the cut, and the result says so.
    let own = [(8.5, 8.5), (8.7, 8.7), (9.0, 9.0)];
    for (n, cut) in [(16u32, false), (17, true)] {
        let (doc, r) = shapes_round_a_box(n);
        for own in [&own[..], &[]] {
            let out = ask(&doc, &vector_settings(), r, own, 5.5);
            assert_eq!(out.truncated, cut, "{n} shapes, {} own points: {out:?}", own.len());
        }
    }
}

/// A shape with a square drawn in its frame (one `Geometry` section of five rows) from (`x0`, `y0`) to (`x1`, `y1`) on the page.
fn drawn_square(id: u32, x0: f64, y0: f64, x1: f64, y1: f64) -> ShapeBuilder {
    let (w, h) = (x1 - x0, y1 - y0);
    let mut shape = ShapeBuilder::new(id)
        .cell("PinX", &((x0 + x1) / 2.0).to_string())
        .cell("PinY", &((y0 + y1) / 2.0).to_string())
        .cell("Width", &w.to_string())
        .cell("Height", &h.to_string())
        .cell("LocPinX", &(w / 2.0).to_string())
        .cell("LocPinY", &(h / 2.0).to_string())
        .cell("Angle", "0");
    for (i, (x, y)) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0)].iter().enumerate() {
        let row = RowKey::Ix(u32::try_from(i).unwrap());
        let kind = if i == 0 { "MoveTo" } else { "LineTo" };
        shape = shape.row_type("Geometry", Some(0), row.clone(), kind).row_cell("Geometry", Some(0), row.clone(), "X", &x.to_string()).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &y.to_string(),
        );
    }
    shape
}

#[test]
fn m2_the_outlines_of_ten_drawn_shapes_are_built_once_a_call_with_places_of_the_moving_shapes_own() {
    // Ten shapes that draw a square each, within reach, no connection point among them: with places of the moving shapes' own the call asks the library twice (the outlines
    // apart) and builds ten outlines, not twenty, which the budget of sixteen would have cut at the seventeenth read.
    let mut page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20");
    for i in 0..10u32 {
        let (x, y) = (8.0 + f64::from(i % 5) * 0.3, 8.0 + f64::from(i / 5) * 0.3);
        page = page.shape(drawn_square(i + 1, x, y, x + 0.2, y + 0.2));
    }
    let doc = DocBuilder::new().page(1, page).build();
    let own = [(8.5, 8.5), (8.7, 8.7), (9.0, 9.0)];
    let out = ask(&doc, &vector_settings(), Rect { x0: 8.2, y0: 8.2, x1: 9.2, y1: 9.2 }, &own, 5.5);
    assert!(!out.truncated, "{out:?}");
    assert_eq!(out.outline.built, 10, "each outline is built once: {:?}", out.outline);
    // Without them the call asks once and builds the same ten.
    let out = ask(&doc, &vector_settings(), Rect { x0: 8.2, y0: 8.2, x1: 9.2, y1: 9.2 }, &[], 5.5);
    assert_eq!(out.outline.built, 10, "{:?}", out.outline);
}

// ---------------------------------------------------------------------------------------------------------------- shared helpers

/// A session holding `doc`, with the grid and the ruler off, as the spec's vectors have them.
pub fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    run(&mut s, "snap.set", json!({"grid": false, "ruler": false}));
    s
}

pub fn box_in(s: &Session, id: u32) -> Rect {
    box_of(s.document().unwrap(), 1, &[id]).unwrap()
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A master of one shape that draws a line of `rows` rows, and `n` instances of it turned 0.5 radians, 1 by 1 inch, on a grid of places.
pub fn big_master_page(n: u32, rows: u32) -> (MasterBuilder, PageBuilder) {
    let mut master_shape = ShapeBuilder::new(1).cell("Width", "1").cell("Height", "1").cell("LocPinX", "0.5").cell("LocPinY", "0.5");
    for i in 0..rows {
        let (row, t) = (RowKey::Ix(i), if i == 0 { "MoveTo" } else { "LineTo" });
        let v = (f64::from(i % 100) / 100.0).to_string();
        master_shape = master_shape.row_type("Geometry", Some(0), row.clone(), t).row_cell("Geometry", Some(0), row.clone(), "X", &v).row_cell(
            "Geometry",
            Some(0),
            row,
            "Y",
            &v,
        );
    }
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for i in 0..n {
        page = page.shape(
            ShapeBuilder::new(100 + i)
                .master(1)
                .cell("PinX", &(f64::from(i % 50) * 3.0 + 2.0).to_string())
                .cell("PinY", &(f64::from(i / 50) * 3.0 + 2.0).to_string())
                .cell("Width", "1")
                .cell("Height", "1")
                .cell("LocPinX", "0.5")
                .cell("LocPinY", "0.5")
                .cell("Angle", "0.5"),
        );
    }
    (MasterBuilder::new().shape(master_shape), page)
}

// ---------------------------------------------------------------------------------------------------------------- M4, a spent page budget is said in the result

/// `n` turned instances of a master of 256 rows (131,072 points: the page's whole budget at 512 of them), then the stock ellipse turned 30 degrees at (200, 200).
pub fn budget_spent_by(n: u32) -> (Document, u32) {
    let (master, page) = big_master_page(n, 256);
    let mut doc = DocBuilder::new().master(1, master).page(1, page).build();
    let ellipse = drop_shape(&mut doc, 1, find_master("ellipse").unwrap(), 200.0, 200.0).unwrap();
    rotate_shapes(&mut doc, 1, &[ellipse.id], Turn::To(30.0_f64.to_radians())).unwrap();
    (doc, ellipse.id)
}

const BESIDE_THE_ELLIPSE: Rect = Rect { x0: 202.0, y0: 199.0, x1: 203.0, y1: 200.0 };

#[test]
fn a_turned_target_the_page_budget_could_not_pay_for_is_said_in_the_result() {
    // The review's page: 512 turned instances of 256 rows spend the page's 131,072 points (65,536 at the 16 times it was before a change, and 256 of them), and the turned ellipse
    // after them keeps the box round its corners, which was silent (`snap.at` gave no snap and `truncated: false`). With 511 the ellipse is paid for, and nothing is cut.
    let (spent, _) = budget_spent_by(512);
    assert!(ask(&spent, &vector_settings(), BESIDE_THE_ELLIPSE, &[], 96.0).truncated);
    let (paid, _) = budget_spent_by(511);
    assert!(!ask(&paid, &vector_settings(), BESIDE_THE_ELLIPSE, &[], 96.0).truncated);
}

#[test]
fn a_page_of_five_thousand_turned_shapes_none_of_which_the_budget_can_pay_for_says_so() {
    // The review's second page: 5,000 turned instances of a master of 4,000 rows (a section over the cap of rows): every one falls back to its corners.
    let (master, page) = big_master_page(5_000, 4_000);
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    assert!(ask(&doc, &vector_settings(), BESIDE_THE_ELLIPSE, &[], 96.0).truncated);
    // The same kind of page with 500 instances of a master of 100 rows (50,000 points of the page's 131,072) is paid for in full: not a cut.
    let (master, page) = big_master_page(500, 100);
    let doc = DocBuilder::new().master(1, master).page(1, page).build();
    assert!(!ask(&doc, &vector_settings(), BESIDE_THE_ELLIPSE, &[], 96.0).truncated);
}

#[test]
fn a_turned_shape_that_draws_no_path_is_no_cut_of_the_budget_and_so_is_not_said() {
    // A turned shape with no geometry keeps the box round its corners because there is nothing to draw, which costs nothing.
    let page = PageBuilder::new().cell("PageWidth", "20").cell("PageHeight", "20").shape(
        ShapeBuilder::new(1)
            .cell("PinX", "5")
            .cell("PinY", "5")
            .cell("Width", "2")
            .cell("Height", "1")
            .cell("LocPinX", "1")
            .cell("LocPinY", "0.5")
            .cell("Angle", "0.5"),
    );
    let doc = DocBuilder::new().page(1, page).build();
    let out = ask(&doc, &vector_settings(), Rect { x0: 7.0, y0: 5.0, x1: 8.0, y1: 6.0 }, &[], 96.0);
    assert!(!out.truncated, "{out:?}");
}

#[test]
fn a_moving_shape_the_budget_could_not_pay_for_is_no_target_and_is_not_said_and_neither_is_a_snap_that_reads_no_shape() {
    let (doc, ellipse) = budget_spent_by(512);
    let page = SnapPage::read(&doc, 1).unwrap();
    let ids = [ellipse];
    let q = |moving| Query { subject: Subject::Box(BESIDE_THE_ELLIPSE), using: Use::Place, moving, zoom: 96.0, own_points: &[] };
    assert!(!snap(&doc, &page, &vector_settings(), &q(&ids)).truncated, "the ellipse moves: it is no target");
    assert!(snap(&doc, &page, &vector_settings(), &q(&[])).truncated, "the ellipse is a target");
    // A grid-only snap reads no shape at all.
    let mut grid_only = SnapSettings::none();
    grid_only.snap = true;
    grid_only.grid = true;
    assert!(!snap(&doc, &page, &grid_only, &q(&[])).truncated);
}
