//! The cells of other pages that read a written cell (gap F40): a commit for an edit of page 1 works out the cells of page 2 that name a cell of page 1 through
//! `Pages[..]`, and what reads those in turn, on any page, in the same commit and on the same evaluator, so that the cached values are the ones a full evaluation gives. The pages that
//! read are found by a listing of the formulas that name a page, kept in the [`ReaderIndex`] from one edit to the next and put right by what each edit replaced.

use std::sync::Arc;

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, ReaderIndex, Since, changes_between_with, commit_changed_across, recalc_all};
use diacraft_doc::{CellAddr, CellEdit, Document, Lookup, Owner, RowKey, Shape, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, PageBuilder, ShapeBuilder};

fn width_addr() -> CellAddr {
    CellAddr::direct("Width")
}

fn shape_sheet(page: u32, place: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(page), place))
}

/// The text of the value a cell of a top shape of a page holds (its saved `V`).
fn saved(doc: &Document, page: u32, place: u32, addr: &CellAddr) -> Option<String> {
    let shape = doc.shape(&ShapeRef::top(Owner::Page(page), place))?;
    match shape.sheet.lookup(addr) {
        Lookup::Found(cell) => cell.value.as_ref().map(|v| v.text().to_owned()),
        _ => None,
    }
}

fn width(doc: &Document, page: u32, place: u32) -> Option<String> {
    saved(doc, page, place, &width_addr())
}

/// A page named `Page-<id>`, with shape 10 holding the formula of its width (or a constant, when there is no formula) and the saved value.
fn page_with(width_formula: Option<&str>, saved: &str) -> PageBuilder {
    let shape = match width_formula {
        Some(f) => ShapeBuilder::new(10).formula("Width", f, saved),
        None => ShapeBuilder::new(10).cell("Width", saved),
    };
    PageBuilder::new().shape(shape)
}

/// A chain of `n` pages: the width of page 1 is 1, and that of each later page is that of the one before, plus 1.
fn chain(n: u32) -> Document {
    let mut b = DocBuilder::new();
    for p in 1..=n {
        let page = if p == 1 { page_with(None, "1") } else { page_with(Some(&format!("Pages[Page-{}]!Sheet.10!Width+1", p - 1)), &p.to_string()) };
        b = b.page(p, page);
    }
    b.build()
}

/// Writes `formula` to the width of the first shape of `page`, the way an editing command does, and says what it changed on the page.
fn write_width(doc: &mut Document, page: u32, text: &str) -> (Document, Changes) {
    let before = doc.clone();
    doc.write_cell(&shape_sheet(page, 0), &width_addr(), CellEdit::Value(DocValue::from_text(text)), WriteOrigin::Drawing).expect("a write");
    let changes = changes_between_with(&mut ReaderIndex::new(), &before, doc, page);
    (before, changes)
}

fn commit(index: &mut ReaderIndex, doc: &mut Document, page: u32, changes: &Changes) -> CommitReport {
    commit_changed_across(index, doc, &Budget::default(), EvalOptions::default(), page, changes)
}

/// A timed twin of a counted case: `#[ignore]`d, and run by hand on a quiet machine (`--ignored --nocapture`), for the time a person would wait for it.
macro_rules! timed_twin {
    ($name:ident, $seconds:expr, $case:ident) => {
        #[test]
        #[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
        fn $name() {
            let started = std::time::Instant::now();
            $case();
            let took = started.elapsed();
            eprintln!("took {took:?}");
            assert!(took < std::time::Duration::from_secs($seconds), "took {took:?}, more than {} s", $seconds);
        }
    };
}

/// The document as a full evaluation gives it: no cell of a page differs from its cached value.
fn agrees_with_a_full_evaluation(doc: &Document) {
    let report = recalc_all(doc, &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "{:?}", report.differed);
}

#[test]
fn a_cell_that_reads_another_page_by_its_name_follows_a_write_to_the_cell_it_reads() {
    let mut doc = chain(2);
    let (_, changes) = write_width(&mut doc, 1, "5");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("6"), "{report:?}");
    assert_eq!(report.written, 1, "the one cell that reads it: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// `Pages[2]` is a number: a page by `NameU`, which a test names by the digits.
#[test]
fn a_page_named_by_a_number_is_a_page_by_its_name_too() {
    let mut doc =
        DocBuilder::new().page(1, page_with(None, "1").name("2")).page(2, page_with(Some("Pages[2]!Sheet.10!Width*10"), "10").name("Reader")).build();
    let (_, changes) = write_width(&mut doc, 1, "3");
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("30"));
    agrees_with_a_full_evaluation(&doc);
}

#[test]
fn a_chain_through_pages_is_followed_to_its_end_in_one_commit() {
    let mut doc = chain(40);
    let (_, changes) = write_width(&mut doc, 1, "100");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("101"));
    assert_eq!(width(&doc, 40, 0).as_deref(), Some("139"));
    assert_eq!(report.written, 39, "each page's cell once: {report:?}");
    assert!(report.stats.cells_worked <= 80, "each cell is worked out once, not once for each cell that reads it: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A cell of another page that reads a cell of the page's own sheet: `Pages[One]!PageWidth`.
#[test]
fn a_cell_that_reads_the_sheet_of_another_page_follows_it() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("One").cell("PageWidth", "8.5"))
        .page(2, PageBuilder::new().name("Two").shape(ShapeBuilder::new(10).formula("Width", "Pages[One]!PageWidth*2", "17")))
        .build();
    let before = doc.clone();
    doc.write_cell(&SheetRef::Page(1), &CellAddr::direct("PageWidth"), CellEdit::Value(DocValue::from_text("11")), WriteOrigin::Drawing)
        .expect("a write");
    let changes = changes_between_with(&mut ReaderIndex::new(), &before, &doc, 1);
    assert_eq!(changes.cells, vec![(SheetRef::Page(1), CellAddr::direct("PageWidth"))]);
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("22"));
}

/// The cells of the other page that read the one that read page 1 follow it, as they follow a cell of their own page that changed.
#[test]
fn the_cells_of_the_reading_page_that_read_the_cell_that_changed_follow_it_in_turn() {
    let reader = ShapeBuilder::new(10).formula("Width", "Pages[Page-1]!Sheet.10!Width+1", "2");
    let second = ShapeBuilder::new(11).formula("Height", "Sheet.10!Width*2", "4").formula("PinX", "Width+Height", "6");
    let mut doc = DocBuilder::new().page(1, page_with(None, "1")).page(2, PageBuilder::new().shape(reader).shape(second)).build();
    let (_, changes) = write_width(&mut doc, 1, "5");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("6"));
    assert_eq!(saved(&doc, 2, 1, &CellAddr::direct("Height")).as_deref(), Some("12"), "{report:?}");
    assert_eq!(saved(&doc, 2, 1, &CellAddr::direct("PinX")).as_deref(), Some("12"), "a cell of the same shape that reads the cell that changed");
    agrees_with_a_full_evaluation(&doc);
}

/// **A cell passes the change on only if it changed**, across pages: a reader that comes out as it was ends the chain, and a cell of another page that reads nothing that changed keeps
/// the value its file saved, whatever this evaluator gives for it.
#[test]
fn a_reader_that_comes_out_as_it_was_passes_nothing_on_and_a_cell_that_reads_nothing_that_changed_keeps_its_value() {
    let reads_a_clamp = ShapeBuilder::new(10).formula("Width", "MIN(Pages[Page-1]!Sheet.10!Width,3)", "3");
    let follows = ShapeBuilder::new(11).formula("Height", "Sheet.10!Width*2", "9");
    let stale = ShapeBuilder::new(12).formula("Height", "2+2", "9");
    let mut doc =
        DocBuilder::new().page(1, page_with(None, "5")).page(2, PageBuilder::new().shape(reads_a_clamp).shape(follows).shape(stale)).build();
    let (_, changes) = write_width(&mut doc, 1, "8");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("3"), "the reader is still 3");
    assert_eq!(saved(&doc, 2, 1, &CellAddr::direct("Height")).as_deref(), Some("9"), "what reads the reader is not worked out: {report:?}");
    assert_eq!(
        saved(&doc, 2, 2, &CellAddr::direct("Height")).as_deref(),
        Some("9"),
        "a cell that names no cell that changed keeps what its file said"
    );
    assert_eq!(report.written, 0, "{report:?}");
}

/// A cell that is not read by any other page leaves the other pages alone, so a document with no formula that names a page costs one page's work.
#[test]
fn a_drawing_whose_formulas_name_no_page_works_out_the_page_alone() {
    let mut b = DocBuilder::new();
    for p in 1..=8_u32 {
        b = b.page(
            p,
            PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")).shape(ShapeBuilder::new(11).formula(
                "Height",
                "Sheet.10!Width*2",
                "2",
            )),
        );
    }
    let mut doc = b.build();
    let (_, changes) = write_width(&mut doc, 3, "4");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 3, &changes);
    assert_eq!(saved(&doc, 3, 1, &CellAddr::direct("Height")).as_deref(), Some("8"));
    assert_eq!(report.written, 1, "{report:?}");
    for page in [1_u32, 2, 4, 5, 6, 7, 8] {
        assert_eq!(saved(&doc, page, 1, &CellAddr::direct("Height")).as_deref(), Some("2"), "page {page}");
    }
}

/// An edit that works a whole page out (a style, a master, the document's sheet) follows the cells of other pages that read it too.
#[test]
fn a_pass_over_a_whole_page_follows_the_cells_of_other_pages_that_read_it() {
    let rate = "Pages[Page-1]!Sheet.10!Width*2";
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Width", rate, "2")))
        .page(3, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Width", "Pages[Page-2]!Sheet.10!Width+1", "3")))
        .build();
    let before = doc.clone();
    doc.write_cell(&shape_sheet(1, 0), &width_addr(), CellEdit::Value(DocValue::from_text("10")), WriteOrigin::Drawing).expect("a write");
    let changes = Changes { whole_page: true, since: Since::of(&before), ..Changes::default() };
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("20"));
    assert_eq!(width(&doc, 3, 0).as_deref(), Some("21"), "and what reads that");
    agrees_with_a_full_evaluation(&doc);
}

/// A shape taken out of the page it was on: a cell of another page that named it reads nothing now.
#[test]
fn a_cell_that_read_a_shape_that_was_taken_out_is_worked_out() {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")).shape(ShapeBuilder::new(11).cell("Width", "7")))
        .page(2, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Width", "Pages[Page-1]!Sheet.11!Width+1", "8")))
        .build();
    let before = doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    page.shapes = page.shapes.iter().filter(|s| s.id != 11).cloned().collect();
    let changes = changes_between_with(&mut ReaderIndex::new(), &before, &doc, 1);
    assert_eq!(changes.removed, vec![11]);
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let shape = doc.shape(&ShapeRef::top(Owner::Page(2), 0)).expect("the reader");
    let Lookup::Found(cell) = shape.sheet.lookup(&width_addr()) else { panic!("the cell") };
    assert!(cell.error.is_some(), "the reference finds no shape, and the cell says so: {cell:?}");
}

/// The listing is kept from one edit to the next and put right by what each edit replaced: a formula that is written to read another page is followed from then on, and one that
/// is written to read nothing is not.
#[test]
fn the_listing_that_is_kept_follows_the_formulas_that_were_written_since() {
    let mut index = ReaderIndex::new();
    let mut doc = chain(3);
    let (_, changes) = write_width(&mut doc, 1, "5");
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(width(&doc, 3, 0).as_deref(), Some("7"));
    // Page 3 now reads page 1 itself, and no longer page 2.
    let before = doc.clone();
    let formula = diacraft_doc::Cell::with_formula("Pages[Page-1]!Sheet.10!Width*10", Some(DocValue::from_text("50")));
    doc.write_cell(&shape_sheet(3, 0), &width_addr(), CellEdit::Cell(formula), WriteOrigin::Automation).expect("a write");
    let changes = changes_between_with(&mut index, &before, &doc, 3);
    commit(&mut index, &mut doc, 3, &changes);
    assert_eq!(width(&doc, 3, 0).as_deref(), Some("50"));
    let (_, changes) = write_width(&mut doc, 1, "2");
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("3"));
    assert_eq!(width(&doc, 3, 0).as_deref(), Some("20"), "it reads page 1 now");
    // And page 2 no longer reaches page 3.
    let (_, changes) = write_width(&mut doc, 2, "40");
    commit(&mut index, &mut doc, 2, &changes);
    assert_eq!(width(&doc, 3, 0).as_deref(), Some("20"), "page 3 reads page 1 and not page 2");
    agrees_with_a_full_evaluation(&doc);
}

/// A shape put in front of the one a cell of another page names moves the place the shape is in: the reference finds it where it is.
#[test]
fn a_shape_added_before_the_one_that_is_read_does_not_lose_its_readers() {
    let mut index = ReaderIndex::new();
    let mut doc = chain(2);
    let before = doc.clone();
    let page = Arc::make_mut(doc.pages.get_mut(0).expect("a page"));
    page.shapes.insert(0, Arc::new(Shape::new(5))).expect("room for a shape");
    let changes = changes_between_with(&mut index, &before, &doc, 1);
    commit(&mut index, &mut doc, 1, &changes);
    // Now shape 10 is the second shape of page 1.
    let (_, changes) = {
        let before = doc.clone();
        doc.write_cell(&shape_sheet(1, 1), &width_addr(), CellEdit::Value(DocValue::from_text("9")), WriteOrigin::Drawing).expect("a write");
        (before.clone(), changes_between_with(&mut index, &before, &doc, 1))
    };
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(width(&doc, 2, 0).as_deref(), Some("10"));
}

/// A chain longer than the budget is stopped by it, said so in the report, and leaves what it had worked out: never a loop.
fn a_chain_past_the_budget() {
    let mut doc = chain(3_000);
    let (_, changes) = write_width(&mut doc, 1, "5");
    let report = commit_changed_across(&mut ReaderIndex::new(), &mut doc, &Budget::with_steps(50_000), EvalOptions::default(), 1, &changes);
    assert!(report.budget_exhausted, "{report:?}");
    assert!(report.stats.steps <= 50_000, "{report:?}");
    assert!(report.written < 2_999, "the chain was cut short: {report:?}");
}

#[test]
fn a_chain_past_the_budget_stops_at_it() {
    a_chain_past_the_budget();
}

timed_twin!(a_chain_past_the_budget_is_timed, 20, a_chain_past_the_budget);

/// The work is linear in the pages: a chain of twice the pages costs about twice the steps.
#[test]
fn the_steps_of_a_chain_grow_with_its_pages_and_not_with_their_square() {
    let steps = |n: u32| {
        let mut doc = chain(n);
        let (_, changes) = write_width(&mut doc, 1, "5");
        commit(&mut ReaderIndex::new(), &mut doc, 1, &changes).stats.steps
    };
    let (a, b) = (steps(100), steps(200));
    assert!(b <= a * 3, "100 pages {a} steps, 200 pages {b} steps");
}

/// A formula that names ten thousand different pages is listed (each name once, by a set and not by a search of a list) and worked out in steps of its own size: a hostile formula costs what its
/// tree costs and no more, whatever the number of pages it names.
fn a_formula_that_names_ten_thousand_pages() {
    const NAMES: u32 = 10_000;
    let refs: Vec<String> = (1..=NAMES).map(|n| format!("Pages[{n}]!Sheet.10!Width")).collect();
    let formula = format!("SUM({})", refs.join(","));
    let mut doc = DocBuilder::new().page(1, page_with(None, "5").name("1")).page(2, page_with(Some(&formula), "5").name("Reader")).build();
    let (_, changes) = write_width(&mut doc, 1, "7");
    let mut index = ReaderIndex::new();
    let report = commit(&mut index, &mut doc, 1, &changes);
    assert!(!report.budget_exhausted, "{report:?}");
    assert!(report.stats.steps <= 60 * u64::from(NAMES), "a count of steps that is the formula's: {report:?}");
    let shape = doc.shape(&ShapeRef::top(Owner::Page(2), 0)).expect("the reader");
    let Lookup::Found(cell) = shape.sheet.lookup(&width_addr()) else { panic!("the cell") };
    assert!(cell.error.is_some(), "the pages that are not there read #REF!, and the cell says so: {cell:?}");
    // Written again, the listing is kept: the second write costs a fraction of the first.
    let (_, changes) = write_width(&mut doc, 1, "9");
    let again = commit(&mut index, &mut doc, 1, &changes);
    assert!(again.stats.steps <= report.stats.steps, "{again:?} against {report:?}");
}

#[test]
fn a_formula_that_names_ten_thousand_pages_costs_steps_of_its_size() {
    a_formula_that_names_ten_thousand_pages();
}

timed_twin!(a_formula_that_names_ten_thousand_pages_is_timed, 20, a_formula_that_names_ten_thousand_pages);

/// Two pages: page 1 holds a width, and the shape of page 2 holds a `SETF` that writes twice that width into a cell of its own sheet.
fn a_setf_that_reads_another_page() -> Document {
    let go = "SETF(GETREF(Scratch.A1),Pages[Page-1]!Sheet.10!Width*2)";
    DocBuilder::new()
        .page(1, PageBuilder::new().shape(ShapeBuilder::new(10).cell("Width", "1")))
        .page(
            2,
            PageBuilder::new().shape(ShapeBuilder::new(10).row_cell("Scratch", None, RowKey::Ix(0), "A", "0").row_formula(
                "User",
                None,
                RowKey::name("Go"),
                "Value",
                go,
                "0",
            )),
        )
        .build()
}

/// What a `SETF` of another page that the commit of an edit reaches writes is worked out as the write of any call is: the value of the cell it wrote is the formula's, not the 0 it held
/// (the pass of the write was for the page of the edit, and took a cell of another page for a reason to work that page out whole).
#[test]
fn a_setf_of_another_page_that_an_edit_reaches_has_its_write_worked_out() {
    let mut doc = a_setf_that_reads_another_page();
    let (_, changes) = write_width(&mut doc, 1, "10");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let target = CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A");
    assert_eq!(saved(&doc, 2, 0, &target).as_deref(), Some("20"), "the call wrote its target and the cell holds what it gives: {report:?}");
    assert_eq!(report.setf_written, 1, "{report:?}");
}

/// A `SETF` on another page that reads the page worked out whole writes in the second commit, and its writes are in the report (the engine's note of what
/// the calls did is made from it). The two fixes were made apart, and the second commit's report was added by hand, which left the counts of the `SETF` writes out.
#[test]
fn a_setf_of_another_page_that_the_second_commit_works_out_is_counted_in_the_report() {
    let mut doc = a_setf_that_reads_another_page();
    let before = doc.clone();
    doc.write_cell(&shape_sheet(1, 0), &width_addr(), CellEdit::Value(DocValue::from_text("10")), WriteOrigin::Drawing).expect("a write");
    let changes = Changes { whole_page: true, since: Since::of(&before), ..Changes::default() };
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let target = CellAddr::in_row("Scratch", None, RowKey::Ix(0), "A");
    assert_eq!(saved(&doc, 2, 0, &target).as_deref(), Some("20"), "the call wrote its target: {report:?}");
    assert_eq!(report.setf_written, 1, "and the report says so: {report:?}");
}
