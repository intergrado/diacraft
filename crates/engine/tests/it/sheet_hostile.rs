//! Chained and hostile inputs for the ShapeSheet commands: the five cases of the design's section 11, "Chained and hostile", and the others the plan adds (long chains across
//! pages, loops of `SETF`, renumber storms, formulas at the caps, pages that pass the budget), each held to a **count**: the steps the evaluator took and the commits it made
//! (`diacraft_calc::work`), never a time. A twin of each that is timed is `#[ignore]`d and run by hand (`--ignored --nocapture`).
//!
//! **The budget** (asked this task to settle it): the evaluator's steps are 20 million for the whole of a call, **shared by the pages it works out** (`calc_index::SharedSteps`):
//! `sheet.set_cell` on the document's sheet or a style reaches every page, and a drawing of many pages each just under 20 million was worked out in as many budgets, which a file made for it
//! holds for hours. A page that passes what is left, or the pages together, make the call refuse in words with nothing changed. The drawings of the tests that follow are the largest the
//! model holds in a command (64 pages of 2,000 shapes, 100,000 cells that read one, 4,096 writes read by 100 cells each) and stay far under it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use diacraft_calc::{Budget, recalc_all, work};
use diacraft_doc::{Cell, CellAddr, Document, RowKey, Value as Held};
use diacraft_engine::Session;
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};
use serde_json::{Value, json};

use crate::{blank_session, check_session, run, snapshot};

/// A session whose document is `doc`.
fn session_of(doc: Document) -> Session {
    let mut s = blank_session();
    s.edit("build", |d| {
        *d = doc;
        Ok(())
    })
    .unwrap();
    s
}

/// What a call cost, as counts: the steps the evaluator took and the commits it made.
#[derive(Clone, Copy, Debug)]
struct Cost {
    steps: u64,
    commits: u64,
}

/// The most steps of one call: the budget of the evaluator, and the little it may go past it by (a text it was reading when the budget was spent).
const BUDGET: u64 = Budget::DEFAULT_STEPS;
const SLACK: u64 = 100_000;

/// Whether the budget was spent and no more: the work stops within a text's charge of it (it is charged before it is read, so it ends a little under).
fn spent(cost: &Cost) -> bool {
    cost.steps + SLACK >= BUDGET && cost.steps <= BUDGET + SLACK
}

/// Runs a command that must succeed and counts its work.
fn counted(s: &mut Session, id: &str, params: Value) -> (Value, Cost) {
    work::reset();
    let answer = run(s, id, params);
    (answer, Cost { steps: work::steps(), commits: work::commits() })
}

/// Runs a command that may be refused and counts its work.
fn counted_try(s: &mut Session, id: &str, params: Value) -> (Result<Value, String>, Cost) {
    work::reset();
    let answer = s.execute(id, &params).map_err(|e| e.to_string());
    (answer, Cost { steps: work::steps(), commits: work::commits() })
}

/// The drawing and what the history holds, to say that a call changed nothing.
fn state(s: &Session) -> (Option<Document>, usize, usize, u64) {
    let snap = snapshot(s);
    (snap.document, snap.undo_steps, snap.redo_steps, snap.revision)
}

/// A call that must be refused whole, in the words it gives: nothing changed.
fn refused_whole(s: &mut Session, id: &str, params: Value, words: &str) -> Cost {
    let anchor = state(s);
    let (answer, cost) = counted_try(s, id, params);
    let e = answer.expect_err("the call is refused");
    assert!(e.contains(words), "{e}");
    assert!(state(s) == anchor, "a refusal changes nothing");
    cost
}

/// Runs `case` and says it took less than `limit`: the timed twin of a counted test, `#[ignore]`d, for a quiet machine.
fn timed(limit: Duration, case: impl FnOnce()) {
    let started = Instant::now();
    case();
    let took = started.elapsed();
    eprintln!("took {took:?}");
    assert!(took < limit, "took {took:?}, more than {limit:?}");
}

/// A timed twin of a counted case: `#[ignore]`d, and run by hand on a quiet machine (`--ignored --nocapture`), for the time a person would wait for it.
macro_rules! timed_twin {
    ($name:ident, $seconds:expr, $case:ident) => {
        #[test]
        #[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
        fn $name() {
            timed(Duration::from_secs($seconds), $case);
        }
    };
}

/// The sheet's `Cell` for a constant.
fn constant(text: &str) -> Cell {
    Cell::constant(Held::from_text(text))
}

/// Gives the document's sheet its `User.Rate` of `text`.
fn with_rate(mut d: Document, text: &str) -> Document {
    Arc::make_mut(&mut d.sheet).set_cell(&CellAddr::in_row("User", None, RowKey::name("Rate"), "Value"), constant(text)).unwrap();
    d
}

// ---- one cell that many cells read ------------------------------------------------------------------------------------------------------

const READERS: u32 = 100_000;

/// Shape 10 holds a `Width` and 100,000 Scratch rows that each read it.
fn a_cell_read_by_many() -> Session {
    let mut shape = ShapeBuilder::new(10).cell_u("Width", "2", "IN");
    for k in 0..READERS {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", "Width*2", "4");
    }
    session_of(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build())
}

/// A write to a cell that 100,000 cells read works them out in one commit, in a count of steps that is theirs (17 each), lists 256 of them and says there are more; and undo puts them all back.
fn a_write_to_a_cell_that_100000_cells_read() {
    let mut s = a_cell_read_by_many();
    let anchor = state(&s).0;
    let (answer, cost) = counted(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Width", "formula": "3"}));
    let r = &answer["recalculated"];
    assert_eq!((r["cells"].clone(), r["listed"].as_array().map(Vec::len), r["more"].clone()), (json!(READERS), Some(256), json!(true)), "{r}");
    assert_eq!(cost.commits, 1, "one commit for the page");
    assert!(cost.steps >= u64::from(READERS), "each reader is worked out: {cost:?}");
    assert!(cost.steps <= 25 * u64::from(READERS), "a few tens of steps for each reader and no more: {cost:?}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Scratch.A100000"}))["value"], json!(6.0), "the last reader follows");
    assert_eq!(check_session(&s), Ok(()));
    run(&mut s, "edit.undo", json!({}));
    assert!(state(&s).0 == anchor, "undo puts every reader back");
}

#[test]
fn a_write_to_a_cell_that_100000_cells_read_is_one_commit_of_a_few_steps_each() {
    a_write_to_a_cell_that_100000_cells_read();
}

timed_twin!(a_write_to_a_cell_that_100000_cells_read_is_timed, 30, a_write_to_a_cell_that_100000_cells_read);

// ---- a chain of cells that each read the one before ---------------------------------------------------------------------------------------

const LINKS: u32 = 10_000;

/// Shape 10 holds 10,000 Scratch rows, each `A` one more than the row before.
fn a_chain() -> Session {
    let mut shape = ShapeBuilder::new(10).row_cell("Scratch", None, RowKey::Ix(0), "A", "1");
    for k in 1..LINKS {
        shape = shape.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Scratch.A{k}+1"), &(k + 1).to_string());
    }
    session_of(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build())
}

/// A write at the head of a chain of 10,000 cells works the others out in one commit and 18 steps for each, the last of them reads the head and 9,999, and nothing ends in a stack too deep.
fn a_write_at_the_head_of_a_chain_of_10000_cells() {
    let mut s = a_chain();
    let (answer, cost) = counted(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Scratch.A1", "formula": "5"}));
    assert_eq!(answer["recalculated"]["cells"], LINKS - 1, "{answer}");
    assert_eq!(cost.commits, 1);
    assert!(cost.steps >= u64::from(LINKS) && cost.steps <= 30 * u64::from(LINKS), "{cost:?}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"id": 10, "cell": "Scratch.A10000"}))["value"], json!(10_004.0));
    let report = recalc_all(s.document().unwrap(), &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "{:?}", report.differed);
    // The middle of it, written again, works out the half after it only.
    let (answer, half) = counted(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Scratch.A5000", "formula": "0"}));
    assert_eq!(answer["recalculated"]["cells"], LINKS - 5_000, "{answer}");
    assert!(half.steps < cost.steps, "the half costs less than the whole: {half:?} against {cost:?}");
}

#[test]
fn a_write_at_the_head_of_a_chain_of_10000_cells_is_one_commit_of_a_few_steps_each() {
    a_write_at_the_head_of_a_chain_of_10000_cells();
}

timed_twin!(a_write_at_the_head_of_a_chain_of_10000_cells_is_timed, 20, a_write_at_the_head_of_a_chain_of_10000_cells);

// ---- the document's sheet, and many pages -----------------------------------------------------------------------------------------------

/// A drawing of `pages` pages of `shapes` shapes each, every shape reading the document's `User.Rate` and its own width: what a write to the document's sheet works out whole.
fn pages_of_shapes(pages: u32, shapes: u32) -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=pages {
        let mut page = PageBuilder::new().name(&format!("Page-{p}"));
        for k in 0..shapes {
            let shape = ShapeBuilder::new(10 + k).cell_u("Width", "2", "IN").formula("Height", "TheDoc!User.Rate.Value*1", "2").formula(
                "LocPinX",
                "Width*0.5",
                "1",
            );
            page = page.shape(shape);
        }
        b = b.page(p, page);
    }
    session_of(with_rate(b.build(), "2"))
}

/// The steps of a write to the document's `User.Rate` in a drawing of `pages` pages of `shapes` shapes, and the commits it made.
fn document_write(pages: u32, shapes: u32) -> Cost {
    let mut s = pages_of_shapes(pages, shapes);
    let (answer, cost) = counted(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate.Value", "formula": "3"}));
    assert_eq!(answer["recalculated"]["cells"], u64::from(pages * shapes), "each shape's Height follows: {answer}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": pages, "id": 10 + shapes - 1, "cell": "Height"}))["value"], json!(3.0));
    cost
}

/// A write to the document's sheet in a drawing of 64 pages of 2,000 shapes works out each page once, whole, in steps that are the shapes' (33 each, two cells), a fifth of the budget.
fn a_write_to_the_documents_sheet_over_64_pages_of_2000_shapes() {
    let cost = document_write(64, 2_000);
    assert_eq!(cost.commits, 64, "one commit for each page");
    assert!(cost.steps <= 64 * 2_000 * 50, "{cost:?}");
    assert!(cost.steps < BUDGET / 2, "far under what a call may take: {cost:?}");
}

#[test]
fn a_write_to_the_documents_sheet_works_out_64_pages_of_2000_shapes_once_each() {
    a_write_to_the_documents_sheet_over_64_pages_of_2000_shapes();
}

timed_twin!(a_write_to_the_documents_sheet_over_64_pages_of_2000_shapes_is_timed, 60, a_write_to_the_documents_sheet_over_64_pages_of_2000_shapes);

/// **No page costs more because of the others**: a write to the document's sheet costs the sum of the pages' own costs, so that two, four, eight, sixteen and thirty-two pages cost
/// two, four, eight, sixteen and thirty-two times one (within five in a hundred). This is the count that says the total across pages is the sum of what the pages hold, so that the
/// budget of a call, shared by them, is the budget of a drawing, and that nothing in the work grows with the number of pages.
#[test]
fn the_steps_of_a_write_to_the_documents_sheet_are_the_sum_of_what_its_pages_cost() {
    let one = document_write(1, 100);
    assert_eq!(one.commits, 1);
    for pages in [2_u32, 4, 8, 16, 32] {
        let cost = document_write(pages, 100);
        assert_eq!(cost.commits, u64::from(pages), "one commit for each page");
        assert!(cost.steps <= u64::from(pages) * one.steps * 105 / 100, "{pages} pages: {cost:?} against one page {one:?}");
        assert!(cost.steps >= u64::from(pages) * one.steps * 95 / 100, "{pages} pages: {cost:?} against one page {one:?}");
    }
}

/// A chain through the pages, each page's `Width` reading the one before through `Pages[..]`, rooted in the document's cell, is followed in page order by a write to the document's
/// sheet: each page is worked out once, on the values the page before it was given. A reference by a page's name looks the name up among the pages and is charged for it, so the cost of
/// the chain grows with the square of the pages (741 steps a page at 64 pages), which is why the budget is the call's and not each page's: a thousand pages that each hold a hundred
/// such references cost more than a call may spend, and are refused whole.
fn a_chain_through_64_pages() {
    const PAGES: u32 = 64;
    let mut b = DocBuilder::new();
    for p in 1..=PAGES {
        let formula = if p == 1 { "TheDoc!User.Rate.Value".to_owned() } else { format!("Pages[Page-{}]!Sheet.10!Width+1", p - 1) };
        b = b.page(p, PageBuilder::new().name(&format!("Page-{p}")).shape(ShapeBuilder::new(10).formula("Width", &formula, &p.to_string())));
    }
    let mut s = session_of(with_rate(b.build(), "1"));
    let (_, cost) = counted(&mut s, "sheet.set_cell", json!({"document": true, "cell": "User.Rate.Value", "formula": "100"}));
    assert_eq!(cost.commits, u64::from(PAGES));
    assert!(cost.steps <= u64::from(PAGES * PAGES) * 20, "no worse than a lookup of the page's name among the pages for each page: {cost:?}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 1, "id": 10, "cell": "Width"}))["value"], json!(100.0));
    assert_eq!(
        run(&mut s, "sheet.cell", json!({"page": PAGES, "id": 10, "cell": "Width"}))["value"],
        json!(163.0),
        "the last page reads 63 pages of +1"
    );
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_chain_through_64_pages_rooted_in_the_documents_cell_is_followed_in_page_order() {
    a_chain_through_64_pages();
}

timed_twin!(a_chain_through_64_pages_is_timed, 20, a_chain_through_64_pages);

/// A chain through the pages rooted in a **shape**: page `p` holds a shape 10 whose `Width` reads the one of page `p - 1` through `Pages[..]`.
fn a_chain_of_200_pages_rooted_in_a_shape() -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=200_u32 {
        let shape = if p == 1 {
            ShapeBuilder::new(10).cell("Width", "1")
        } else {
            ShapeBuilder::new(10).formula("Width", &format!("Pages[Page-{}]!Sheet.10!Width+1", p - 1), &p.to_string())
        };
        b = b.page(p, PageBuilder::new().name(&format!("Page-{p}")).shape(shape));
    }
    session_of(b.build())
}

/// A write to the shape on page 1 of a chain of 200 pages works out the 199 cells that read it, one on each later page, in **one commit and a count of steps that grows with the pages**
/// (gap F40): each is worked out once on the evaluator of the first, and a page's own graph is made when the change first reaches it. The cells are listed with the
/// others the call worked out.
#[test]
fn a_write_to_a_shape_on_a_page_of_a_chain_of_200_pages_works_out_the_later_pages_in_one_commit() {
    let mut s = a_chain_of_200_pages_rooted_in_a_shape();
    let (answer, cost) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    eprintln!("a chain of 200 pages: {cost:?}");
    assert_eq!(cost.commits, 1, "one commit, which follows the chain from page to page: {cost:?}");
    assert!(cost.steps <= 200 * 200, "at most what 200 pages of one cell cost: {cost:?}");
    let r = &answer["recalculated"];
    assert_eq!(
        (r["cells"].clone(), r["listed"].as_array().map(Vec::len), r["more"].clone()),
        (json!(199), Some(199), json!(false)),
        "one cell on each later page: {r}"
    );
    assert_eq!(check_session(&s), Ok(()));
}

/// **The vector of F40**: the cells of other pages that read a cell of a page through `Pages[..]` are worked out by a write to it, so that the last page of a chain holds the value a full
/// evaluation gives.
#[test]
fn a_write_to_a_shape_works_out_the_cells_of_other_pages_that_read_it() {
    let mut s = a_chain_of_200_pages_rooted_in_a_shape();
    run(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 2, "id": 10, "cell": "Width"}))["value"], json!(6.0));
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 200, "id": 10, "cell": "Width"}))["value"], json!(204.0));
    let report = recalc_all(s.document().unwrap(), &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "a full evaluation finds nothing to change: {:?}", report.differed);
}

// ---- cells that read other pages: 64 pages of 2,000 shapes -----------------------------------------------------------------------------------

/// A drawing of `pages` pages of `shapes` shapes each (as `pages_of_shapes`, 33 steps a shape), where the first shape of each page after the first reads the first of the page before it
/// through `Pages[..]`: a chain through every page, over pages of the largest size the model holds in a command.
fn chained_pages_of_shapes(pages: u32, shapes: u32) -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=pages {
        let mut page = PageBuilder::new().name(&format!("Page-{p}"));
        for k in 0..shapes {
            let mut shape = ShapeBuilder::new(10 + k).formula("Height", "TheDoc!User.Rate.Value*1", "2").formula("LocPinX", "Width*0.5", "1");
            shape = if k == 0 && p > 1 {
                shape.formula("Width", &format!("Pages[Page-{}]!Sheet.10!Width+1", p - 1), &p.to_string())
            } else {
                shape.cell_u("Width", "2", "IN")
            };
            page = page.shape(shape);
        }
        b = b.page(p, page);
    }
    session_of(with_rate(b.build(), "2"))
}

/// A write to the first shape of a chain through 64 pages of 2,000 shapes works the 63 cells that read it out, each on its own page, in one commit and a **tenth of the budget**: the page of the
/// write is worked out as it always was, a page a change reaches has its graph made then (the 6,000 formulas of its shapes asked whether they read beyond their sheet), and the
/// listing of the sheets that name a page is made once, by a pass over the text of the 384,000 formulas of the drawing, which the writes after it do not repeat.
fn a_write_through_a_chain_over_64_pages_of_2000_shapes() {
    let mut s = chained_pages_of_shapes(64, 2_000);
    let (answer, first) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    eprintln!("64 pages of 2,000 shapes, chained: the first write {first:?}");
    assert_eq!(first.commits, 1, "one commit: {first:?}");
    // The 63 widths of the later pages, and the `LocPinX` of the 64 pages that reads its own width: the cells that read a cell that changed, and no others.
    assert_eq!(answer["recalculated"]["cells"], json!(63 + 64), "{}", answer["recalculated"]);
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 64, "id": 10, "cell": "Width"}))["value"], json!(68.0), "the last page reads 63 pages of +1");
    assert!(first.steps < BUDGET / 4, "far under what a call may take: {first:?}");
    // The graphs and the listing are kept: the next write costs what it touched, and the pages are not made again.
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "7"}));
    eprintln!("64 pages of 2,000 shapes, chained: the second write {second:?}");
    assert_eq!(second.commits, 1);
    assert!(second.steps * 4 < first.steps, "the second write is a fraction of the first: {second:?} against {first:?}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 64, "id": 10, "cell": "Width"}))["value"], json!(70.0));
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_through_a_chain_over_64_pages_of_2000_shapes_is_one_commit_within_the_budget() {
    a_write_through_a_chain_over_64_pages_of_2000_shapes();
}

timed_twin!(a_write_through_a_chain_over_64_pages_of_2000_shapes_is_timed, 60, a_write_through_a_chain_over_64_pages_of_2000_shapes);

/// A drawing whose formulas name no page costs no more than it did to write to a page of it: the first write makes the listing of the sheets that name a page (a step for 64 bytes of formula
/// text, and none of the cells), and the next write finds it kept.
fn a_write_in_a_drawing_whose_formulas_name_no_page() {
    let mut s = pages_of_shapes(64, 2_000);
    let (_, first) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    let (_, second) = counted(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "6"}));
    eprintln!("64 pages of 2,000 shapes, no formula names a page: the first write {first:?}, the second {second:?}");
    assert!(first.steps < 64 * 2_000 * 2, "the first write: a step a shape for the listing of 64 pages: {first:?}");
    assert!(second.steps < 100, "the second finds the listing kept, and works out the one shape: {second:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_write_in_a_drawing_of_64_pages_whose_formulas_name_no_page_costs_the_page_and_a_listing_once() {
    a_write_in_a_drawing_whose_formulas_name_no_page();
}

timed_twin!(a_write_in_a_drawing_whose_formulas_name_no_page_is_timed, 30, a_write_in_a_drawing_whose_formulas_name_no_page);

// ---- the budget: a page that passes it, and pages that pass it together -------------------------------------------------------------------

/// The text a reader cell reads many times: each read of it is charged its length, 64 bytes a step (`rules::UNITS_PER_STEP`), though no byte is copied.
const BIG: usize = 524_288;

/// A page whose shape 10 holds a text of `BIG` bytes in `User.Big.Value` and a cell that reads it `reads` times (`BIG` / 64 = 8,192 steps a read), and reads the document's `Rate`.
fn heavy_page(reads: usize) -> PageBuilder {
    let sum = format!("SUM({})", vec!["LEN(User.Big.Value)"; reads].join(","));
    let shape = ShapeBuilder::new(10)
        .row_cell("User", None, RowKey::name("Big"), "Value", &"x".repeat(BIG))
        .row_formula("User", None, RowKey::name("Reader"), "Value", &sum, &(BIG * reads).to_string())
        .formula("Height", "TheDoc!User.Rate.Value*1", "2");
    PageBuilder::new().shape(shape)
}

/// A drawing of `pages` heavy pages, each of `reads` reads of the big text.
fn heavy_drawing(pages: u32, reads: usize) -> Session {
    let mut b = DocBuilder::new();
    for p in 1..=pages {
        b = b.page(p, heavy_page(reads).name(&format!("Page-{p}")));
    }
    session_of(with_rate(b.build(), "2"))
}

fn rate_params() -> Value {
    json!({"document": true, "cell": "User.Rate.Value", "formula": "3"})
}

/// A page that passes the budget by itself (3,000 reads of 8,192 steps are 24.6 million) is refused whole, in words, having spent the budget and not a step more than a text, and the
/// drawing, the history and the revision are as they were.
fn a_page_that_passes_the_budget() {
    let mut s = heavy_drawing(1, 3_000);
    let cost = refused_whole(&mut s, "sheet.set_cell", rate_params(), "too large to work out again");
    assert!(spent(&cost), "the budget was spent and no more: {cost:?}");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn a_page_that_passes_the_budget_is_refused_whole_in_words_having_spent_the_budget_and_no_more() {
    a_page_that_passes_the_budget();
}

timed_twin!(a_page_that_passes_the_budget_is_timed, 30, a_page_that_passes_the_budget);

/// The same page with a third of the reads (1,000: 8.2 million steps) is worked out, within the budget and well past what a shape costs.
#[test]
fn a_page_within_the_budget_is_worked_out_alone() {
    let mut s = heavy_drawing(1, 1_000);
    let (answer, cost) = counted_try(&mut s, "sheet.set_cell", rate_params());
    answer.expect("a page of 8 million steps is within the budget");
    assert_eq!(cost.commits, 1);
    assert!((8_000_000..=9_000_000).contains(&cost.steps), "{cost:?}");
}

/// **The pages of a call share the budget.** Six pages of 8.2 million steps each are each within the budget and 49 million together: the call is refused whole after the third (the
/// budget is spent in it), having worked out no more than the budget and a text, and nothing is changed. Six pages of 2.5 million are 14.8 million together, and are worked out.
fn pages_that_pass_the_budget_together() {
    let mut s = heavy_drawing(6, 1_000);
    let cost = refused_whole(&mut s, "sheet.set_cell", rate_params(), "too large to work out again");
    assert!(spent(&cost), "the budget of the call, shared, and no more: {cost:?}");
    assert!(cost.commits <= 4, "the pages after the budget is spent are not worked out: {cost:?}");
    // Within it, the same number of pages is worked out, each once.
    let mut s = heavy_drawing(6, 300);
    let (answer, cost) = counted_try(&mut s, "sheet.set_cell", rate_params());
    answer.expect("six pages of 2.5 million steps are within the budget they share");
    assert_eq!(cost.commits, 6);
    assert!(cost.steps < BUDGET, "{cost:?}");
}

#[test]
fn pages_that_each_pass_less_than_the_budget_and_more_together_are_refused_whole_in_words() {
    pages_that_pass_the_budget_together();
}

timed_twin!(pages_that_pass_the_budget_together_are_timed, 60, pages_that_pass_the_budget_together);

/// A chain of `pages` heavy pages: the first page's shape 10 holds a width of 1, and the width of each later page's shape 10 is the one before it, plus 1, through `Pages[..]`, and 0 times the shape's own
/// `User.Reader`, which reads a text of `BIG` bytes `reads` times (8,192 steps a read).
fn heavy_chain(pages: u32, reads: usize) -> Session {
    let sum = format!("SUM({})", vec!["LEN(User.Big.Value)"; reads].join(","));
    let mut b = DocBuilder::new();
    for p in 1..=pages {
        let mut shape = ShapeBuilder::new(10).row_cell("User", None, RowKey::name("Big"), "Value", &"x".repeat(BIG)).row_formula(
            "User",
            None,
            RowKey::name("Reader"),
            "Value",
            &sum,
            &(BIG * reads).to_string(),
        );
        shape = if p == 1 {
            shape.cell("Width", "1")
        } else {
            shape.formula("Width", &format!("Pages[Page-{}]!Sheet.10!Width+1+0*User.Reader.Value", p - 1), &p.to_string())
        };
        b = b.page(p, PageBuilder::new().name(&format!("Page-{p}")).shape(shape));
    }
    session_of(b.build())
}

/// **A hostile chain through the pages is refused at the budget, in words, and never hangs.** Six pages whose cells each cost 8.2 million steps are each within the budget and 49 million
/// together: a write to the first is refused whole after the third (the budget of the call is spent in it, as it is for pages worked whole), having worked out no more than the budget and a
/// text, and the drawing, the history and the revision are as they were. Six pages of 2.5 million are 15 million together, and are worked out in one commit.
fn a_chain_through_pages_that_passes_the_budget() {
    let mut s = heavy_chain(6, 1_000);
    let cost = refused_whole(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}), "too large to work out again");
    eprintln!("a chain of heavy pages past the budget: {cost:?}");
    assert!(spent(&cost), "the budget of the call and no more: {cost:?}");
    assert_eq!(check_session(&s), Ok(()));
    // Within the budget the same chain is worked out, each page once, in the one commit.
    let mut s = heavy_chain(6, 300);
    let (answer, cost) = counted_try(&mut s, "sheet.set_cell", json!({"page": 1, "id": 10, "cell": "Width", "formula": "5"}));
    answer.expect("six pages of 2.5 million steps are within the budget they share");
    assert_eq!(cost.commits, 1, "{cost:?}");
    assert!((12_000_000..14_000_000).contains(&cost.steps), "five pages of 2.5 million steps: {cost:?}");
    assert_eq!(run(&mut s, "sheet.cell", json!({"page": 6, "id": 10, "cell": "Width"}))["value"], json!(10.0));
}

#[test]
fn a_chain_through_pages_that_passes_the_budget_is_refused_whole_in_words() {
    a_chain_through_pages_that_passes_the_budget();
}

timed_twin!(a_chain_through_pages_that_passes_the_budget_is_timed, 60, a_chain_through_pages_that_passes_the_budget);

// ---- a batch: 4,096 writes, each read by 100 cells ----------------------------------------------------------------------------------------

/// Shape 10 holds 4,096 Scratch rows; 100 shapes each hold 4,096 rows that read the row of the same number: each of the 4,096 cells is read by 100 cells.
fn cells_read_by_a_hundred_each() -> Session {
    let mut targets = ShapeBuilder::new(10);
    for k in 0..4_096_u32 {
        targets = targets.row_cell("Scratch", None, RowKey::Ix(k), "A", &k.to_string());
    }
    let mut page = PageBuilder::new().shape(targets);
    for j in 0..100_u32 {
        let mut reader = ShapeBuilder::new(11 + j);
        for k in 0..4_096_u32 {
            reader = reader.row_formula("Scratch", None, RowKey::Ix(k), "A", &format!("Sheet.10!Scratch.A{}*2", k + 1), &(2 * k).to_string());
        }
        page = page.shape(reader);
    }
    session_of(DocBuilder::new().page(1, page).build())
}

/// 4,096 writes in one batch, each cell read by 100 cells, are one step and one commit of the page, the readers worked out once each (409,600 of them, 24 steps each): all or nothing.
fn a_batch_of_4096_writes_each_read_by_100_cells() {
    let mut s = cells_read_by_a_hundred_each();
    let anchor = state(&s).0;
    let writes: Vec<Value> = (0..4_096_u32).map(|k| json!({"cell": format!("Scratch.A{}", k + 1), "formula": (k + 1000).to_string()})).collect();
    let (answer, cost) = counted(&mut s, "sheet.set_cells", json!({"id": 10, "writes": writes}));
    assert_eq!((answer["written"].clone(), answer["recalculated"]["cells"].clone()), (json!(4_096), json!(409_600)), "{}", answer["recalculated"]);
    assert_eq!(cost.commits, 1, "one commit for the page, at the end");
    assert!(cost.steps >= 409_600 && cost.steps <= 409_600 * 30, "{cost:?}");
    assert_eq!(
        run(&mut s, "sheet.cell", json!({"id": 110, "cell": "Scratch.A4096"}))["value"],
        json!(2.0 * 5_095.0),
        "the last reader of the last write"
    );
    run(&mut s, "edit.undo", json!({}));
    assert!(state(&s).0 == anchor, "one undo step takes the whole batch back");
}

#[test]
fn a_batch_of_4096_writes_each_read_by_100_cells_is_one_commit_of_a_few_steps_each() {
    a_batch_of_4096_writes_each_read_by_100_cells();
}

timed_twin!(a_batch_of_4096_writes_each_read_by_100_cells_is_timed, 60, a_batch_of_4096_writes_each_read_by_100_cells);

// ---- formulas at the caps ---------------------------------------------------------------------------------------------------------------

/// A formula of about `bytes` bytes whose brackets nest `depth` deep, the innermost a call of `MAX` over as many 1s as fill it.
fn nested(depth: usize, bytes: usize) -> String {
    let (open, close) = ("(".repeat(depth - 1), ")".repeat(depth - 1));
    let (head, tail) = (format!("{open}MAX("), format!("1){close}"));
    let fill = bytes.saturating_sub(head.len() + tail.len());
    format!("{head}{}{tail}", "1,".repeat(fill / 2))
}

fn one_cell() -> Session {
    let shape = ShapeBuilder::new(10).cell("Width", "2").row_cell("User", None, RowKey::name("T"), "Value", "1");
    session_of(DocBuilder::new().page(1, PageBuilder::new().shape(shape)).build())
}

/// Writes `text` to `User.T.Value` of shape 10: its value when it is worked out, or the words that refused it, with the steps it took; the drawing is as it was after a refusal.
fn write_t(s: &mut Session, text: &str) -> (Result<Value, String>, Cost) {
    let anchor = state(s);
    let (answer, cost) = counted_try(s, "sheet.set_cell", json!({"id": 10, "cell": "User.T.Value", "formula": text}));
    assert!(answer.is_ok() || state(s) == anchor, "a refusal changes nothing");
    assert!(cost.steps <= BUDGET + SLACK, "{cost:?}");
    (answer.map(|a| a["value"].clone()), cost)
}

/// A formula of 65,536 bytes with its brackets nested to the parser's limit is written and worked out; one level more is refused in words, and so are a tree one level too deep and a
/// text one byte too long. The formulas that fit are worked out in steps that are their parts (about 1.5 a byte) and no more.
fn formulas_at_the_caps() {
    let mut s = one_cell();
    let max = diacraft_formula::MAX_NESTING;
    let (at, cost) = write_t(&mut s, &nested(max, 65_536));
    assert_eq!(at, Ok(json!(1.0)), "nested {max} deep, 65,536 bytes: {cost:?}");
    assert!(cost.steps <= 2 * 65_536, "{cost:?}");
    let (past, _) = write_t(&mut s, &nested(max + 1, 65_536));
    assert!(past.expect_err("one level too deep").contains("nested too deeply"));
    // A left-leaning chain at the depth of the tree.
    let depth = diacraft_formula::MAX_DEPTH;
    let (at, _) = write_t(&mut s, &vec!["1"; depth].join("+"));
    assert_eq!(at, Ok(json!(depth as f64)));
    let (past, _) = write_t(&mut s, &vec!["1"; depth + 1].join("+"));
    assert!(past.expect_err("one term too many").contains("nested too deeply"));
    // The widest: 32,000 arguments, and 10,900 references to a cell.
    let (wide, cost) = write_t(&mut s, &format!("MAX({}1)", "1,".repeat(32_000)));
    assert_eq!(wide, Ok(json!(1.0)));
    assert!(cost.steps <= 3 * 65_536, "{cost:?}");
    let (named, cost) = write_t(&mut s, &format!("MAX({}Width)", "Width,".repeat(10_900)));
    assert_eq!(named, Ok(json!(2.0)));
    assert!(cost.steps <= 2 * 65_536, "{cost:?}");
    // One byte past the limit of a formula.
    let (long, cost) = write_t(&mut s, &format!("1{}", " ".repeat(65_536)));
    assert!(long.expect_err("65,537 bytes").contains("at most 65536 bytes"));
    assert_eq!(cost.steps, 0, "refused before any work");
    assert_eq!(check_session(&s), Ok(()));
}

#[test]
fn formulas_at_the_caps_are_written_and_worked_out_and_one_past_them_is_refused_in_words() {
    formulas_at_the_caps();
}

timed_twin!(formulas_at_the_caps_are_timed, 20, formulas_at_the_caps);

/// A formula of 60,000 bytes that reads a text of half a mebibyte 3,000 times parses, and is then worked out by the page, which passes the budget: the call is refused whole (the
/// formula is not written), in words. 1,000 reads are worked out.
fn a_formula_at_the_cap_that_reads_a_big_text() {
    let mut s = session_of(DocBuilder::new().page(1, heavy_page(1).name("Page-1")).build());
    let sum = |reads: usize| format!("SUM({})", vec!["LEN(User.Big.Value)"; reads].join(","));
    let cost =
        refused_whole(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "User.Reader.Value", "formula": sum(3_000)}), "too large to work out again");
    assert!(spent(&cost), "{cost:?}");
    let (answer, cost) = counted_try(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "User.Reader.Value", "formula": sum(1_000)}));
    assert_eq!(answer.map(|a| a["value"].clone()), Ok(json!(BIG as f64 * 1_000.0)));
    assert!((8_000_000..=9_000_000).contains(&cost.steps), "{cost:?}");
}

#[test]
fn a_formula_at_the_cap_that_reads_a_big_text_is_refused_whole_where_it_passes_the_budget() {
    a_formula_at_the_cap_that_reads_a_big_text();
}

timed_twin!(a_formula_at_the_cap_that_reads_a_big_text_is_timed, 30, a_formula_at_the_cap_that_reads_a_big_text);

// ---- loops of SETF ----------------------------------------------------------------------------------------------------------------------

/// Two shapes with the cells the loops use: Scratch cells that name each other through `SETF`, one that names itself, a width that does, and a loop between the heights of the two shapes,
/// each reading a cell that a later write changes (`Scratch.D1`), so that each is worked out again.
fn setf_loops() -> Session {
    let shape = ShapeBuilder::new(10)
        .cell("Width", "2")
        .row_cell("Scratch", None, RowKey::Ix(0), "A", "1")
        .row_cell("Scratch", None, RowKey::Ix(0), "B", "2")
        .row_cell("Scratch", None, RowKey::Ix(0), "C", "3")
        .row_cell("Scratch", None, RowKey::Ix(0), "D", "0");
    let other = ShapeBuilder::new(11).cell("Width", "2");
    session_of(DocBuilder::new().page(1, PageBuilder::new().shape(shape).shape(other)).build())
}

const LOOPS: [(u32, &str, &str); 6] = [
    (10, "Scratch.A1", "SETF(GetRef(Scratch.B1),\"Scratch.A1+1\")+Scratch.D1"),
    (10, "Scratch.B1", "SETF(GetRef(Scratch.A1),\"Scratch.B1+1\")+Scratch.D1"),
    (10, "Scratch.C1", "SETF(GetRef(Scratch.C1),\"Scratch.C1+1\")+Scratch.D1"),
    (10, "Width", "SETF(GetRef(Width),\"Width+1\")+1+Scratch.D1"),
    (10, "Height", "SETF(GetRef(Sheet.11!Height),\"Sheet.10!Height+1\")+Scratch.D1"),
    (11, "Height", "SETF(GetRef(Sheet.10!Height),\"Sheet.11!Height+1\")+Sheet.10!Scratch.D1"),
];

/// Whatever a loop of `SETF` does when it is worked out, the call ends: it is written and worked out, or refused whole in words, **within the budget of a call** (a count of steps), the
/// drawing is one a writer saves, and undo takes it back to what it was. The test holds whichever way the evaluator performs `SETF` (it writes nothing today, and may write when it is
/// worked out): it asserts no outcome of the loop but that it ends, counted.
fn loops_of_setf() {
    let mut s = setf_loops();
    let start = state(&s).0;
    for (id, cell, text) in LOOPS {
        let anchor = state(&s);
        let (answer, cost) = counted_try(&mut s, "sheet.set_cell", json!({"id": id, "cell": cell, "formula": text}));
        assert!(cost.steps <= BUDGET + SLACK, "{cell}: {cost:?}");
        if let Err(words) = answer {
            assert!(!words.is_empty() && state(&s) == anchor, "{cell}: refused in words, changing nothing: {words}");
        }
        assert_eq!(check_session(&s), Ok(()), "{cell}");
    }
    // Each loop worked out again, one write after another and then in one batch.
    for k in 1..=3 {
        let (answer, cost) = counted_try(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Scratch.D1", "formula": k.to_string()}));
        assert!(cost.steps <= BUDGET + SLACK, "{cost:?}");
        if let Err(words) = answer {
            assert!(!words.is_empty(), "refused in words");
        }
        assert_eq!(check_session(&s), Ok(()));
    }
    let writes: Vec<Value> = LOOPS.iter().map(|(id, cell, text)| json!({"id": id, "cell": cell, "formula": format!("{text}+1")})).collect();
    let (_, cost) = counted_try(&mut s, "sheet.set_cells", json!({"writes": writes}));
    assert!(cost.steps <= BUDGET + SLACK, "{cost:?}");
    assert_eq!(check_session(&s), Ok(()));
    // Everything the loops did is taken back by undo, a step at a time, to the drawing that began.
    while s.history().is_some_and(|h| h.undo_len() > 1) {
        run(&mut s, "edit.undo", json!({}));
    }
    assert!(state(&s).0 == start, "undo took every write back");
}

#[test]
fn loops_of_setf_end_within_the_budget_of_a_call_and_undo_takes_them_back() {
    loops_of_setf();
}

timed_twin!(loops_of_setf_are_timed, 30, loops_of_setf);

/// A `SETF` in an action cell is never worked out, whatever it names: its text is parsed and the cell is written, with no step of the evaluator beyond the cell's own.
#[test]
fn a_setf_in_an_action_cell_is_never_worked_out() {
    let mut s = setf_loops();
    run(&mut s, "sheet.add_section", json!({"id": 10, "section": "Actions"}));
    let text = "SETF(GetRef(Actions.Row_1.Action),\"Actions.Row_1.Action\")";
    let (answer, cost) = counted_try(&mut s, "sheet.set_cell", json!({"id": 10, "cell": "Actions.Row_1.Action", "formula": text}));
    answer.expect("an action cell is written");
    assert!(cost.steps <= 100, "no step of the evaluator is spent on an action cell: {cost:?}");
}

// ---- renumber storms --------------------------------------------------------------------------------------------------------------------

const ROWS: u32 = 200;

/// Shape 10 holds `ROWS` Scratch rows (row k holds `A` = 10k and `B` = the row before's A plus 1), and shape 11 `ROWS` rows that each read the `A` of the row of the same number.
fn a_storm_drawing() -> Session {
    let mut rows = ShapeBuilder::new(10);
    let mut readers = ShapeBuilder::new(11);
    for k in 1..=ROWS {
        rows = rows.row_cell("Scratch", None, RowKey::Ix(k - 1), "A", &(10 * k).to_string());
        let b = if k == 1 { ("0".to_owned(), "1".to_owned()) } else { (format!("Scratch.A{}+1", k - 1), (10 * (k - 1) + 1).to_string()) };
        rows = rows.row_formula("Scratch", None, RowKey::Ix(k - 1), "B", &b.0, &b.1);
        readers = readers.row_formula("Scratch", None, RowKey::Ix(k - 1), "A", &format!("Sheet.10!Scratch.A{k}"), &(10 * k).to_string());
    }
    session_of(DocBuilder::new().page(1, PageBuilder::new().shape(rows).shape(readers)).build())
}

/// What one call of the storm reads and writes, counted: the formulas it looks at are those of the page's two sheets and each formula is read once.
fn assert_work(answer: &Value, step: u32) {
    let r = &answer["renumbered"];
    let (formulas, work) = (r["formulas"].as_u64().unwrap_or(u64::MAX), &r["work"]);
    assert!(formulas <= u64::from(3 * ROWS), "step {step}: {r}");
    assert!(work["parsed"].as_u64().unwrap_or(u64::MAX) <= u64::from(3 * ROWS), "step {step}: each formula is read once: {r}");
    assert!(work["visited"].as_u64().unwrap_or(u64::MAX) <= u64::from(8 * ROWS), "step {step}: {r}");
}

/// A storm of 60 inserts at the front, the middle and the end and deletes of rows that formulas name, 600 formulas on two shapes following them: each call's work is counted (the formulas
/// it reads and writes are at most those of the page, and none is parsed twice), every reader still reads the value of the row it named (the row that went, the value it had), a full
/// recalculation finds nothing left over, and one undo step at a time takes the storm back to the drawing that began.
fn a_storm_of_inserts_and_deletes() {
    let mut s = a_storm_drawing();
    let start = state(&s).0;
    // The rows as the model knows them: each tag, in order.
    let mut order: Vec<u32> = (1..=ROWS).collect();
    let mut next_tag = ROWS + 1;
    let mut rng = 0x5EED_u64;
    let mut below = move |n: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        usize::try_from(rng % u64::try_from(n.max(1)).unwrap()).unwrap()
    };
    for step in 0..60 {
        let at = below(order.len() + 1);
        if step % 3 == 2 && order.len() > 2 {
            let at = at.min(order.len() - 1);
            let answer = run(&mut s, "sheet.delete_row", json!({"id": 10, "section": "Scratch", "row": at + 1}));
            order.remove(at);
            assert_work(&answer, step);
        } else {
            let params =
                if at == order.len() { json!({"id": 10, "section": "Scratch"}) } else { json!({"id": 10, "section": "Scratch", "before": at + 1}) };
            let answer = run(&mut s, "sheet.add_row", params);
            assert_eq!(answer["row"], at + 1, "step {step}: {answer}");
            assert_work(&answer, step);
            order.insert(at, next_tag);
            run(&mut s, "sheet.set_cell", json!({"id": 10, "cell": format!("Scratch.A{}", at + 1), "formula": (10 * next_tag).to_string()}));
            next_tag += 1;
        }
    }
    // The rows hold their tags in order; every reader of a row that is there reads its tag.
    for (at, tag) in order.iter().enumerate() {
        let a = run(&mut s, "sheet.cell", json!({"id": 10, "cell": format!("Scratch.A{}", at + 1)}));
        assert_eq!(a["value"], json!(f64::from(10 * tag)), "row {}", at + 1);
    }
    for k in 1..=ROWS {
        let reader = run(&mut s, "sheet.cell", json!({"id": 11, "cell": format!("Scratch.A{k}")}));
        assert_eq!(reader["value"], json!(f64::from(10 * k)), "the reader of the row that began as {k} reads its value, or the one it had: {reader}");
    }
    let report = recalc_all(s.document().unwrap(), &Budget::default());
    assert!(report.differed.is_empty(), "{:?}", report.differed);
    assert_eq!(check_session(&s), Ok(()));
    while s.history().is_some_and(|h| h.undo_len() > 1) {
        run(&mut s, "edit.undo", json!({}));
    }
    assert!(state(&s).0 == start, "undo took the storm back, a step at a time");
}

#[test]
fn a_storm_of_inserts_and_deletes_over_600_formulas_follows_every_row_and_undo_takes_it_back() {
    a_storm_of_inserts_and_deletes();
}

timed_twin!(a_storm_of_inserts_and_deletes_is_timed, 60, a_storm_of_inserts_and_deletes);

/// Every counted case above, one after the other, on a quiet machine, in the time a person would wait for the biggest of them (a target, never a gate: no test asserts a time).
#[test]
#[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
fn the_hostile_cases_finish_in_a_few_minutes() {
    timed(Duration::from_secs(240), || {
        a_write_to_a_cell_that_100000_cells_read();
        a_write_at_the_head_of_a_chain_of_10000_cells();
        a_write_to_the_documents_sheet_over_64_pages_of_2000_shapes();
        a_batch_of_4096_writes_each_read_by_100_cells();
        pages_that_pass_the_budget_together();
        a_page_that_passes_the_budget();
        a_storm_of_inserts_and_deletes();
    });
}
