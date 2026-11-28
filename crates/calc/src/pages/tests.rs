//! What a master that names pages costs the listing, counted (the findings of the security scan): the places the listing holds for an instance, and the cells found for an
//! instance when a change to the pages is worked out, are charged to the commit (its memory and its steps), and a commit that cannot hold them gives the listing up and says so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use diacraft_doc::RowKey;
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder};

use super::*;

/// Master 1 holds a shape 5 whose `Width` reads `names` different pages (a `SUM`) and which has `cells` rows of `Scratch` that each read the page numbered 7; `instances` instances of it sit
/// on page 2. Page 1 is called "1" and holds a shape 3.
fn instances_of_a_master_that_names_pages(names: u32, cells: u32, instances: u32) -> Document {
    let refs: Vec<String> = (1..=names).map(|n| format!("Pages[{n}]!Sheet.3!Width")).collect();
    let mut shape = ShapeBuilder::new(5).formula("Width", &if names == 0 { "0".to_owned() } else { format!("SUM({})", refs.join(",")) }, "0");
    for k in 0..cells {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Pages[7]!Sheet.3!Width+{k}"), "0");
    }
    let mut board = PageBuilder::new().name("Board");
    for k in 0..instances {
        board = board.shape(ShapeBuilder::new(10 + k).master(1));
    }
    DocBuilder::new()
        .master(1, MasterBuilder::new().name("Meter").shape(shape))
        .page(1, PageBuilder::new().name("1").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, board)
        .build()
}

/// The document with a page numbered 7 added, and what that gave another reading.
fn page_seven_is_added(doc: &Document) -> (Document, PageNames) {
    let mut now = doc.clone();
    now.add_page(Some("7")).expect("a page");
    let names = PageNames::between(doc, &now);
    (now, names)
}

fn context(steps: u64, bytes: u64) -> EvalCtx {
    EvalCtx::new(EvalOptions::default(), Budget { steps, bytes })
}

/// The listing of `doc`, made within a budget that holds it.
fn listed(doc: &Document) -> PageLinks {
    let mut ctx = context(Budget::DEFAULT_STEPS, Budget::DEFAULT_BYTES);
    let mut ev = Evaluator::new(doc, &mut ctx);
    let mut links = PageLinks::default();
    links.sync(&mut ev, doc, true);
    assert!(!ev.ctx.stats.budget_exhausted, "{:?}", ev.ctx.stats);
    links
}

/// **The listing is charged for what it holds.** An instance that inherits the names of its master shape is listed under each of them, so a master that names 200 pages and 500 instances of it
/// are 100,000 places in the index of names, and the listing is charged for them, a quarter of the bytes of a place each, and not for the 500 instances alone.
#[test]
fn the_listing_is_charged_for_each_name_each_instance_inherits() {
    const NAMES: u64 = 200;
    const INSTANCES: u64 = 500;
    let (doc, _) = page_seven_is_added(&instances_of_a_master_that_names_pages(NAMES as u32, 0, INSTANCES as u32));
    let links = listed(&doc);
    let places = NAMES * INSTANCES;
    assert!(links.bytes >= places * KEY_BYTES / 4, "{} bytes held for {places} places", links.bytes);
}

/// A commit that cannot hold those places **gives the listing up before it builds them**, within the memory it is given: 2 MiB cannot hold 100,000 places, the commit ends as out of memory,
/// and nothing of the listing is left to hold.
#[test]
fn a_listing_that_passes_the_memory_of_the_commit_is_given_up_before_it_is_built() {
    let (doc, _) = page_seven_is_added(&instances_of_a_master_that_names_pages(200, 0, 500));
    let mut ctx = context(Budget::DEFAULT_STEPS, 2 << 20);
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let mut links = PageLinks::default();
    links.sync(&mut ev, &doc, true);
    assert!(ev.ctx.stats.memory_exhausted, "{:?}", ev.ctx.stats);
    assert!(ev.ctx.stats.bytes_peak <= 2 << 20, "{:?}", ev.ctx.stats);
    assert_eq!((links.bytes, links.by_name.len()), (0, 0), "the listing is let go of, and not built past what the commit holds");
}

/// **The cells an instance inherits are found within the steps of the call**: a master shape of 200 cells that name the page and 500 instances are 100,000 cells to find, a step each, and a walk
/// of 50,000 steps ends as out of budget and hands back no more cells than it had steps for.
#[test]
fn the_cells_instances_inherit_are_found_within_the_steps_of_the_call() {
    let (doc, names) = page_seven_is_added(&instances_of_a_master_that_names_pages(0, 200, 500));
    let links = listed(&doc);
    let mut ctx = context(50_000, Budget::DEFAULT_BYTES);
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let readers = links.readers_naming(&mut ev, &names);
    assert!(ev.ctx.stats.budget_exhausted, "{:?}", ev.ctx.stats);
    assert!(ev.ctx.stats.steps <= 50_000, "{:?}", ev.ctx.stats);
    assert!(readers.len() <= 50_000, "{} cells for 50,000 steps", readers.len());
}

/// The same walk is held to the memory: the cells it finds are each a place held, and a call of 2 MiB ends as out of memory in it, with the cells it could hold.
#[test]
fn the_cells_instances_inherit_are_found_within_the_memory_of_the_call() {
    let (doc, names) = page_seven_is_added(&instances_of_a_master_that_names_pages(0, 200, 500));
    let links = listed(&doc);
    let mut ctx = context(Budget::DEFAULT_STEPS, 2 << 20);
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let readers = links.readers_naming(&mut ev, &names);
    assert!(ev.ctx.stats.memory_exhausted, "{:?}", ev.ctx.stats);
    assert!(ev.ctx.stats.bytes_peak <= 2 << 20, "{:?}", ev.ctx.stats);
    assert!((readers.len() as u64) * KEY_BYTES <= 2 << 20, "{} cells held in 2 MiB", readers.len());
}

/// The cells of a master shape and of each instance of it are each found once.
#[test]
fn the_cells_of_a_master_and_of_its_instances_are_each_found_once() {
    let (doc, names) = page_seven_is_added(&instances_of_a_master_that_names_pages(0, 3, 4));
    let links = listed(&doc);
    let mut ctx = context(Budget::DEFAULT_STEPS, Budget::DEFAULT_BYTES);
    let mut ev = Evaluator::new(&doc, &mut ctx);
    let readers = links.readers_naming(&mut ev, &names);
    let distinct: HashSet<&Reader> = readers.iter().collect();
    assert_eq!(readers.len(), distinct.len());
    // The master's three rows, and each of the four instances' three.
    assert_eq!(readers.len(), 3 + 4 * 3);
}
