//! What the pointer's scene costs a page of shapes whose cells hold a formula and no saved value, counted: the scene works each cell it reads out once, for a
//! revision, in one evaluator, and a click, a rubber band or a frame of a drag works none out; a page whose steps run out is read as saved from there on and is still a scene; a
//! chain of cells too long for a part of the steps is given up on, never waited for.
//!
//! The counts are those of `diacraft_engine::draw::worked`, kept for the thread that builds the scene: a count does not move with the load on the machine, as a time does.

use diacraft_doc::{Cell, CellAddr, Document, RowKey};
use diacraft_engine::Session;
use diacraft_engine::draw::worked::{CHUNK, Counts, counts, reset_counts};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use diacraft_ui_egui::edit::scene::{MAX_ITEMS, Scene};
use serde_json::json;

fn session_with(built: Document) -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit("build", |d| {
        *d = built;
        Ok(())
    })
    .unwrap();
    s
}

/// A page of `n` shapes, 1 by 1, whose `PinX` is a formula and no saved value: shape `id` is at x = `id % 200`.
fn page_of(n: u32) -> Document {
    let mut page = PageBuilder::new().cell("PageWidth", "400").cell("PageHeight", "400");
    for id in 1..=n {
        let shape = ShapeBuilder::new(id)
            .set(CellAddr::direct("PinX"), Cell::with_formula(&format!("{}+0", id % 200), None))
            .cell("PinY", &(f64::from(id / 200) * 1.5 + 1.0).to_string())
            .cell("Width", "1")
            .cell("Height", "1")
            .cell("LocPinX", "0.5")
            .cell("LocPinY", "0.5")
            .cell("Angle", "0");
        page = page.shape(shape);
    }
    DocBuilder::new().page(1, page).build()
}

fn page_id(s: &Session) -> u32 {
    s.document().unwrap().pages.get(0).unwrap().id
}

#[test]
fn a_scene_of_a_page_of_thirty_thousand_such_shapes_works_out_the_cells_it_reads_once_and_a_click_works_out_none() {
    let s = session_with(page_of(30_000));
    let doc = s.document().unwrap();
    reset_counts();
    let scene = Scene::build(doc, page_id(&s), s.revision()).unwrap();
    let built = counts();
    eprintln!("a scene of 30,000 shapes: {built:?}");
    assert_eq!((scene.items.len(), scene.unread), (MAX_ITEMS, 30_000 - MAX_ITEMS), "the scene reads the most it reads");
    assert_eq!(
        (built.worked_out, built.evaluators, built.unanswered),
        (MAX_ITEMS as u64, 1, 0),
        "the pin of each shape read, once, in one evaluator: {built:?}"
    );
    // The shapes are where the formulas put them: shape 7 is at x = 7, its box 6.5 to 7.5.
    let item = scene.get(7).unwrap();
    assert!((item.pin.0 - 7.0).abs() < 1e-9, "{:?}", item.pin);
    // What the pointer does after the build costs no cell: clicks, rubber bands, glue targets, the points of the shapes.
    reset_counts();
    for step in 0..1_000u32 {
        let x = f64::from(step % 200) + 0.5;
        let _ = scene.hit((x, 1.5), 0.05);
        let _ = scene.hit_target((x, 1.5), 0.05);
        let _ = scene.inside((x, 0.0), (x + 3.0, 30.0));
        let _ = scene.points(doc, step % 200 + 1);
    }
    assert_eq!(counts(), Counts::default(), "a click works nothing out");
}

#[test]
fn a_scene_whose_steps_run_out_reads_the_cells_left_as_saved_and_is_still_a_scene() {
    // Steps for some of the 5,000 cells the scene reads only: the shapes whose pin was worked out are where the formula puts them, and the others are at 0, as they were read before.
    let s = session_with(page_of(5_000));
    reset_counts();
    let scene = Scene::build_within(s.document().unwrap(), page_id(&s), s.revision(), 4_000).unwrap();
    let built = counts();
    eprintln!("a scene past its steps: {built:?}");
    assert_eq!(scene.items.len(), 5_000, "every shape is in the scene");
    assert!(built.worked_out > 100 && built.worked_out < 5_000 && built.unanswered > 0, "{built:?}");
    assert!(built.steps <= 4_000, "{built:?}");
    assert!((scene.get(7).unwrap().pin.0 - 7.0).abs() < 1e-9, "the first shapes were answered");
    assert!(scene.get(4_999).unwrap().pin.0.abs() < 1e-9, "the last was not: it reads as saved");
}

#[test]
fn a_scene_of_a_pin_that_is_a_chain_of_cells_too_long_for_a_part_does_not_hang() {
    // 50,000 cells, each the one before and one: some 400,000 steps against a part of 50,000. The shape is at the origin, as a cell with no value reads.
    let user = |name: String| CellAddr::in_row("User", None, RowKey::name(&name), "Value");
    let mut shape = ShapeBuilder::new(1).set(user("C0".to_owned()), Cell::with_formula("1", None));
    for i in 1..=50_000u32 {
        shape = shape.set(user(format!("C{i}")), Cell::with_formula(&format!("User.C{}+1", i - 1), None));
    }
    let shape = shape.set(CellAddr::direct("PinX"), Cell::with_formula("User.C50000", None)).cell("PinY", "1").cell("Width", "1").cell("Height", "1");
    let s = session_with(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build());
    reset_counts();
    let scene = Scene::build(s.document().unwrap(), page_id(&s), s.revision()).unwrap();
    let built = counts();
    assert!(scene.get(1).is_some_and(|item| item.pin.0.abs() < 1e-9), "{:?}", scene.get(1).map(|i| i.pin));
    assert!(built.unanswered >= 1 && built.steps <= CHUNK + 1_000, "given up on at the end of a part: {built:?}");
}

#[test]
#[ignore = "timed: a scene of a page of 30,000 shapes whose pins are formulas and no saved value; the counted tests above hold the work, this the time"]
fn timed_a_scene_of_thirty_thousand_such_shapes_is_built_in_well_under_a_second() {
    let s = session_with(page_of(30_000));
    let started = std::time::Instant::now();
    let scene = Scene::build(s.document().unwrap(), page_id(&s), s.revision()).unwrap();
    let took = started.elapsed();
    eprintln!("a scene of {} shapes: {took:?}", scene.items.len());
    assert!(took < std::time::Duration::from_secs(1), "{took:?}");
}
