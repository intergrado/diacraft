//! The cells that read pages after the pages themselves change, and the formulas the listing of the sheets that name a page did not hold (the rest of gap F40): a page renamed,
//! added, deleted or moved is a name that finds another page, and [`PageNames::between`] says which; a formula a master, a style or the document's sheet holds is listed, and so is the
//! instance that inherits one; a formula that reads at run time (`EVALTEXT`) reads every page, and one that reads the list of pages (`PAGENAME()`) is worked out after a change to it.
//! The counted costs (a master that names five thousand pages, instances of it) have a timed twin that is `#[ignore]`d.

use std::sync::Arc;

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, PageNames, ReaderIndex, changes_between_with, commit_changed_across, recalc_all};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

/// The value and the error a cell of a top shape of a page holds.
fn held(doc: &Document, page: u32, place: u32, addr: &CellAddr) -> (Option<String>, Option<String>) {
    let Some(shape) = doc.shape(&ShapeRef::top(Owner::Page(page), place)) else { return (None, None) };
    match shape.sheet.lookup(addr) {
        Lookup::Found(cell) => (cell.value.as_ref().map(|v| v.text().to_owned()), cell.error.as_ref().map(|e| e.to_string())),
        _ => (None, None),
    }
}

fn width() -> CellAddr {
    CellAddr::direct("Width")
}

fn agrees_with_a_full_evaluation(doc: &Document) {
    let report = recalc_all(doc, &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "{:?}", report.differed);
}

fn rename(doc: &mut Document, id: u32, name: &str) {
    let at = doc.pages.iter().position(|p| p.id == id).expect("the page");
    Arc::make_mut(doc.pages.get_mut(at).expect("the page")).name = Arc::from(name);
}

/// Works out the cells the change from `before` to `doc` gave another reading, as an editing command does.
fn after_the_pages_changed(index: &mut ReaderIndex, doc: &mut Document, before: &Document) -> CommitReport {
    let first = doc.pages.iter().next().map(|p| p.id).expect("a page");
    let changes = Changes { page_names: PageNames::between(before, doc), ..Changes::default() };
    commit_changed_across(index, doc, &Budget::default(), EvalOptions::default(), first, &changes)
}

/// A formula cell with the error a formula that finds no page saved.
fn finds_nothing(formula: &str) -> Cell {
    let mut cell = Cell::with_formula(formula, Some(DocValue::from_text("0")));
    cell.error = Some(Arc::from("#REF!"));
    cell
}

/// Page 1 is "Plan" by its `Name` and "Site-1" by its `NameU`, with a shape 3 of width 5 (place 0); page 2 reads it by the name "Plan".
fn plan_and_reader() -> Document {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10")))
        .build();
    Arc::make_mut(doc.pages.get_mut(0).expect("a page")).name_u = Some(Arc::from("Site-1"));
    doc
}

#[test]
fn two_lists_of_the_same_pages_gave_no_name_another_reading() {
    let doc = plan_and_reader();
    assert!(PageNames::between(&doc, &doc.clone()).is_empty(), "the very pages");
    // Pages that are other allocations and hold the same names, in the same places.
    let mut copy = doc.clone();
    for at in 0..copy.pages.len() {
        let page = diacraft_doc::Page::clone(copy.pages.get(at).expect("a page"));
        copy.pages.set(at, Arc::new(page)).expect("a page");
    }
    assert!(PageNames::between(&doc, &copy).is_empty(), "the same names in the same places");
}

#[test]
fn a_page_renamed_is_a_name_the_cells_that_read_it_find_nothing_by() {
    let before = plan_and_reader();
    let mut doc = before.clone();
    rename(&mut doc, 1, "Dach");
    assert!(!PageNames::between(&before, &doc).is_empty());
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 2, 0, &width()).1.as_deref(), Some("#REF!"), "{report:?}");
    assert_eq!(report.written, 1, "the one cell that names the page: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// The name a formula gives is the page's `NameU` as well as its `Name`: a rename that leaves the `NameU` leaves the page found, and the cell is worked out and comes out as it was.
#[test]
fn a_rename_that_leaves_the_name_a_formula_gives_changes_nothing_a_formula_reads() {
    let mut before = plan_and_reader();
    let formula = Cell::with_formula("Pages[Site-1]!Sheet.3!Width*2", Some(DocValue::from_text("10")));
    before
        .write_cell(&SheetRef::Shape(ShapeRef::top(Owner::Page(2), 0)), &width(), CellEdit::Cell(formula), WriteOrigin::Automation)
        .expect("a write");
    let mut doc = before.clone();
    rename(&mut doc, 1, "Dach");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 2, 0, &width()), (Some("10".to_owned()), None), "{report:?}");
    assert_eq!(report.written, 0, "{report:?}");
}

/// A page deleted down to one leaves the only page's cell that named it reading nothing: a drawing of one page is listed when its pages change.
#[test]
fn a_page_deleted_down_to_one_leaves_the_cell_that_named_it_reading_nothing() {
    let before = plan_and_reader();
    let mut doc = before.clone();
    doc.pages.remove(0).expect("the page");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 2, 0, &width()).1.as_deref(), Some("#REF!"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

#[test]
fn a_page_that_is_added_is_found_by_the_cell_that_named_it() {
    let reader = ShapeBuilder::new(10).set(width(), finds_nothing("Pages[Plan]!PageWidth*2"));
    let before = DocBuilder::new().page(1, PageBuilder::new().name("One").shape(reader)).build();
    let mut doc = before.clone();
    doc.add_page(Some("Plan")).expect("a page");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 1, 0, &width()), (Some("22".to_owned()), None), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// Two pages with one name: the first of the list is the one found, so putting the other first changes the page a name finds.
#[test]
fn a_move_that_changes_which_page_a_name_finds_works_out_the_cells_that_name_it() {
    let before = DocBuilder::new()
        .page(1, PageBuilder::new().name("Dup").shape(ShapeBuilder::new(3).cell("Width", "1")))
        .page(2, PageBuilder::new().name("Dup").shape(ShapeBuilder::new(3).cell("Width", "2")))
        .page(3, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Width", "Pages[Dup]!Sheet.3!Width", "1")))
        .build();
    let mut doc = before.clone();
    let second = doc.pages.get(1).cloned().expect("a page");
    let first = doc.pages.get(0).cloned().expect("a page");
    doc.pages.set(0, second).expect("a page");
    doc.pages.set(1, first).expect("a page");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 3, 0, &width()).0.as_deref(), Some("2"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A page made a background page, or given one, changes what `BKGPAGENAME()` and `PAGECOUNT()` read and no name a formula gives.
#[test]
fn a_cell_that_reads_the_list_of_pages_is_worked_out_after_the_list_changes() {
    let count = ShapeBuilder::new(10).row_formula("User", None, RowKey::name("Count"), "Value", "PAGECOUNT()", "2");
    let before = DocBuilder::new().page(1, PageBuilder::new().name("One")).page(2, PageBuilder::new().name("Two").shape(count)).build();
    let mut doc = before.clone();
    doc.add_page(Some("Three")).expect("a page");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    let addr = CellAddr::in_row("User", None, RowKey::name("Count"), "Value");
    assert_eq!(held(&doc, 2, 0, &addr).0.as_deref(), Some("3"), "{report:?}");
    // A page that is made a background is no page to count.
    let before = doc.clone();
    Arc::make_mut(doc.pages.get_mut(2).expect("a page")).background = true;
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 2, 0, &addr).0.as_deref(), Some("2"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// The value of the cell `Value` of the user row `row` of the first shape of page `page`.
fn user_value(doc: &Document, page: u32, row: &str) -> Option<String> {
    held(doc, page, 0, &CellAddr::in_row("User", None, RowKey::name(row), "Value")).0
}

/// **A page that is only renamed gives another reading to the names and to nothing else**: `PAGENAME()` and `BKGPAGENAME()` are worked out, and a formula that reads only the
/// order or the count of the pages (`PAGENUMBER()`, `PAGECOUNT()`) is not, so the value it holds (a file's own cached value: `5` of `7`) is kept as it is, where a page added, taken away, moved or
/// made a background works them out as before.
#[test]
fn a_page_that_is_only_renamed_works_out_the_cells_that_read_a_name_and_not_those_that_read_the_order_or_the_count() {
    let reader = ShapeBuilder::new(10)
        .row_formula("User", None, RowKey::name("Name"), "Value", "PAGENAME()", "One")
        .row_formula("User", None, RowKey::name("Number"), "Value", "PAGENUMBER()", "5")
        .row_formula("User", None, RowKey::name("Count"), "Value", "PAGECOUNT()", "7")
        .row_formula("User", None, RowKey::name("Back"), "Value", "BKGPAGENAME()", "Backdrop");
    let before = DocBuilder::new()
        .page(1, PageBuilder::new().name("One").back_page(3).shape(reader))
        .page(2, PageBuilder::new().name("Two"))
        .page(3, PageBuilder::new().name("Backdrop").background())
        .build();
    // The page that holds the cells is renamed: its name is read, and the number and the count are not (they are what the page is in the list, which a name does not change).
    let mut doc = before.clone();
    rename(&mut doc, 1, "Plan");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(user_value(&doc, 1, "Name").as_deref(), Some("Plan"), "{report:?}");
    assert_eq!(user_value(&doc, 1, "Number").as_deref(), Some("5"), "the number is kept: {report:?}");
    assert_eq!(user_value(&doc, 1, "Count").as_deref(), Some("7"), "the count is kept: {report:?}");
    assert_eq!(user_value(&doc, 1, "Back").as_deref(), Some("Backdrop"), "{report:?}");
    // The background page is renamed: the name of the background is read, and again no number and no count.
    let before = doc.clone();
    rename(&mut doc, 3, "Frame");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(user_value(&doc, 1, "Back").as_deref(), Some("Frame"), "{report:?}");
    assert_eq!(user_value(&doc, 1, "Number").as_deref(), Some("5"), "{report:?}");
    assert_eq!(user_value(&doc, 1, "Count").as_deref(), Some("7"), "{report:?}");
    // A page added works every one of them out, as it did: the list of pages is another one.
    let before = doc.clone();
    doc.add_page(Some("Three")).expect("a page");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(user_value(&doc, 1, "Number").as_deref(), Some("1"), "{report:?}");
    assert_eq!(user_value(&doc, 1, "Count").as_deref(), Some("3"), "{report:?}");
}

/// What the names and the order of the list of pages gave another reading, as [`PageNames::between`] says it: a rename is the names, and every other change to the pages (one added, taken away, moved, made a
/// background or given another) is the names and the order.
#[test]
fn between_says_a_rename_is_a_change_of_the_names_and_every_other_change_a_change_of_the_order_too() {
    let before = DocBuilder::new().page(1, PageBuilder::new().name("One")).page(2, PageBuilder::new().name("Two")).build();
    let names = |doc: &Document| {
        let changed = PageNames::between(&before, doc);
        (changed.changed_names(), changed.changed_order())
    };
    let mut renamed = before.clone();
    rename(&mut renamed, 2, "Zwei");
    assert_eq!(names(&renamed), (true, false), "a rename");
    let mut universal = before.clone();
    Arc::make_mut(universal.pages.get_mut(0).expect("a page")).name_u = Some(Arc::from("Eins"));
    assert_eq!(names(&universal), (true, false), "a universal name");
    let mut added = before.clone();
    added.add_page(Some("Three")).expect("a page");
    assert_eq!(names(&added), (true, true), "a page added");
    let mut taken = before.clone();
    taken.pages.remove(0).expect("a page");
    assert_eq!(names(&taken), (true, true), "a page taken away");
    let mut moved = before.clone();
    let (first, second) = (moved.pages.get(0).cloned().expect("a page"), moved.pages.get(1).cloned().expect("a page"));
    moved.pages.set(0, second).expect("a page");
    moved.pages.set(1, first).expect("a page");
    assert_eq!(names(&moved), (true, true), "pages moved");
    let mut background = before.clone();
    Arc::make_mut(background.pages.get_mut(1).expect("a page")).background = true;
    assert_eq!(names(&background), (true, true), "a page made a background");
    let mut shown = before.clone();
    Arc::make_mut(shown.pages.get_mut(0).expect("a page")).back_page = Some(2);
    assert_eq!(names(&shown), (true, true), "a page given a background to show");
    // A rename together with a move is both.
    let mut both = moved.clone();
    rename(&mut both, 1, "Eins");
    assert_eq!(names(&both), (true, true), "a rename and a move");
    assert_eq!(names(&before.clone()), (false, false), "the same pages");
}

// ---- the formulas a master, a style and the document's sheet hold ---------------------------------------------------------------------------------

fn write_width(doc: &mut Document, page: u32, text: &str) -> (Document, Changes) {
    let before = doc.clone();
    doc.write_cell(&SheetRef::Shape(ShapeRef::top(Owner::Page(page), 0)), &width(), CellEdit::Value(DocValue::from_text(text)), WriteOrigin::Drawing)
        .expect("a write");
    let changes = changes_between_with(&mut ReaderIndex::new(), &before, doc, page);
    (before, changes)
}

fn commit(index: &mut ReaderIndex, doc: &mut Document, page: u32, changes: &Changes) -> CommitReport {
    commit_changed_across(index, doc, &Budget::default(), EvalOptions::default(), page, changes)
}

/// Master 1 holds a shape 5 whose `Width` reads page "Plan"; page 2 holds an instance of it, which inherits the formula, and holds no formula of its own that names a page.
fn master_that_reads_a_page() -> Document {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).master(1).inh("Width", Some("10"))))
        .build()
}

#[test]
fn an_instance_and_a_master_that_hold_a_formula_that_names_a_page_follow_a_write_to_it() {
    let mut doc = master_that_reads_a_page();
    let (_, changes) = write_width(&mut doc, 1, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(held(&doc, 2, 0, &width()).0.as_deref(), Some("14"), "the instance: {report:?}");
    let master_shape = doc.shape(&ShapeRef::top(Owner::Master(1), 0)).expect("the master's shape");
    let Lookup::Found(cell) = master_shape.sheet.lookup(&width()) else { panic!("the master's cell") };
    assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("14"), "the master's own cell");
    agrees_with_a_full_evaluation(&doc);
}

/// A page renamed is a name the formula an instance inherits does not find either.
#[test]
fn an_instance_that_inherits_a_formula_that_names_a_renamed_page_reads_nothing() {
    let mut before = master_that_reads_a_page();
    Arc::make_mut(before.pages.get_mut(0).expect("a page")).name_u = Some(Arc::from("Site-1"));
    let mut doc = before.clone();
    rename(&mut doc, 1, "Dach");
    let report = after_the_pages_changed(&mut ReaderIndex::new(), &mut doc, &before);
    assert_eq!(held(&doc, 2, 0, &width()).1.as_deref(), Some("#REF!"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

#[test]
fn a_cell_of_a_style_and_one_of_the_documents_sheet_that_name_a_page_follow_a_write_to_it() {
    let style = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let mut doc = DocBuilder::new()
        .style(1, style)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Other"))
        .build();
    let total = CellAddr::in_row("User", None, RowKey::name("Total"), "Value");
    Arc::make_mut(&mut doc.sheet)
        .set_cell(&total, Cell::with_formula("Pages[Plan]!Sheet.3!Width*3", Some(DocValue::from_text("15"))))
        .expect("a cell");
    let (_, changes) = write_width(&mut doc, 1, "7");
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let Lookup::Found(cell) = doc.sheet.lookup(&total) else { panic!("the document's cell") };
    assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("21"));
    let style = doc.styles.iter().find(|s| s.id == 1).expect("the style");
    let Lookup::Found(cell) = style.sheet.lookup(&CellAddr::direct("LineWeight")) else { panic!("the style's cell") };
    assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("14"));
    agrees_with_a_full_evaluation(&doc);
}

/// A drawing of one page has no other page, but the document's sheet reads it all the same.
#[test]
fn the_documents_cell_that_names_the_only_page_follows_it() {
    let mut doc = DocBuilder::new().page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5"))).build();
    let total = CellAddr::in_row("User", None, RowKey::name("Total"), "Value");
    Arc::make_mut(&mut doc.sheet)
        .set_cell(&total, Cell::with_formula("Pages[Plan]!Sheet.3!Width*3", Some(DocValue::from_text("15"))))
        .expect("a cell");
    let (_, changes) = write_width(&mut doc, 1, "7");
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let Lookup::Found(cell) = doc.sheet.lookup(&total) else { panic!("the document's cell") };
    assert_eq!(cell.value.as_ref().map(|v| v.text().to_owned()).as_deref(), Some("21"));
}

/// The listing is kept and put right by what an edit replaced: a master edited to name a page is followed from then on.
#[test]
fn a_master_edited_to_name_a_page_is_followed_by_the_instances_from_then_on() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Height*2", "2"));
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).master(1).inh("Width", Some("2"))))
        .build();
    let mut index = ReaderIndex::new();
    let (_, changes) = write_width(&mut doc, 1, "6");
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(held(&doc, 2, 0, &width()).0.as_deref(), Some("2"), "the master names no page yet");
    // The master's width now reads the page.
    let before = doc.clone();
    let named = Cell::with_formula("Pages[Plan]!Sheet.3!Width*2", Some(DocValue::from_text("12")));
    doc.write_cell(&SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0)), &width(), CellEdit::Cell(named), WriteOrigin::Automation).expect("a write");
    assert!(!before.masters.is_empty());
    let (_, changes) = write_width(&mut doc, 1, "9");
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(held(&doc, 2, 0, &width()).0.as_deref(), Some("18"), "the instance follows the formula it inherits now");
    agrees_with_a_full_evaluation(&doc);
}

// ---- a formula that reads at run time -------------------------------------------------------------------------------------------------------

#[test]
fn a_cell_that_works_a_text_out_as_a_formula_follows_a_write_to_any_other_page() {
    let reader = ShapeBuilder::new(10).text("Pages[Plan]!Sheet.3!Width").formula("Height", "EVALTEXT()", "5");
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(reader))
        .build();
    let (_, changes) = write_width(&mut doc, 1, "9");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(held(&doc, 2, 0, &CellAddr::direct("Height")).0.as_deref(), Some("9"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A cell that reads the members of a list reads the shapes of its own page, which the rule of its page follows: a write to another page does not work it out.
#[test]
fn a_cell_that_reads_a_list_of_its_own_page_is_not_worked_out_by_a_write_to_another() {
    let lister = ShapeBuilder::new(10).formula("Height", "LISTMEMBERCOUNT()+7", "99");
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(lister))
        .build();
    let (_, changes) = write_width(&mut doc, 1, "9");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(held(&doc, 2, 0, &CellAddr::direct("Height")).0.as_deref(), Some("99"), "its saved value stands: {report:?}");
}

// ---- a master that names thousands of pages, and its instances -----------------------------------------------------------------------------

/// A master shape whose width reads five hundred different pages, and three hundred instances of it on one page: the instances **share the names** of their master shape, so the listing
/// holds a place for each instance and the names once, whatever the number of instances, and a write is worked out in steps of the cells it reaches.
fn a_master_that_names_five_hundred_pages() {
    const NAMES: u32 = 500;
    const INSTANCES: u32 = 300;
    let refs: Vec<String> = (1..=NAMES).map(|n| format!("Pages[{n}]!Sheet.3!Width")).collect();
    let formula = format!("SUM({})", refs.join(","));
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", &formula, "0"));
    let mut board = PageBuilder::new().name("Board");
    for k in 0..INSTANCES {
        board = board.shape(ShapeBuilder::new(10 + k).master(1).inh("Width", Some("0")));
    }
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("1").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, board)
        .build();
    let mut index = ReaderIndex::new();
    let (_, changes) = write_width(&mut doc, 1, "7");
    let report = commit(&mut index, &mut doc, 1, &changes);
    assert!(!report.budget_exhausted && !report.memory_exhausted, "{report:?}");
    // The listing is a place for each instance (a few hundred bytes) and the names once, and what a graph of the page holds is the rest.
    assert!(index.held_bytes() < 200 * 1024 * 1024, "{} bytes", index.held_bytes());
    assert!(report.stats.steps <= 400 * u64::from(NAMES * INSTANCES), "{report:?}");
}

#[test]
fn a_master_that_names_five_hundred_pages_is_listed_once_for_all_its_instances() {
    a_master_that_names_five_hundred_pages();
}

#[test]
#[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
fn a_master_that_names_five_hundred_pages_is_timed() {
    let started = std::time::Instant::now();
    a_master_that_names_five_hundred_pages();
    let took = started.elapsed();
    eprintln!("took {took:?}");
    assert!(took < std::time::Duration::from_secs(30), "took {took:?}");
}

/// A master that names five thousand pages and two thousand instances of it are more than a commit holds (each instance is classified for its references): the commit ends **as out of
/// memory, within the memory it is given** (32 MiB here, a small figure so that the test holds little), in a bounded number of steps; the engine says it in words, as the page's refusal.
fn a_master_that_names_five_thousand_pages_passes_the_budget() {
    const NAMES: u32 = 5_000;
    const INSTANCES: u32 = 2_000;
    let refs: Vec<String> = (1..=NAMES).map(|n| format!("Pages[{n}]!Sheet.3!Width")).collect();
    let formula = format!("SUM({})", refs.join(","));
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", &formula, "0"));
    let mut board = PageBuilder::new().name("Board");
    for k in 0..INSTANCES {
        board = board.shape(ShapeBuilder::new(10 + k).master(1).inh("Width", Some("0")));
    }
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("1").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, board)
        .build();
    let (_, changes) = write_width(&mut doc, 1, "7");
    let small = Budget { steps: Budget::DEFAULT_STEPS, bytes: 32 << 20 };
    let report = commit_changed_across(&mut ReaderIndex::new(), &mut doc, &small, EvalOptions::default(), 1, &changes);
    assert!(report.budget_exhausted && report.memory_exhausted, "{report:?}");
    assert!(report.stats.steps <= small.steps, "{report:?}");
    assert!(report.stats.bytes_peak <= small.bytes, "{report:?}");
}

#[test]
fn a_master_that_names_five_thousand_pages_passes_the_budget_and_is_refused_within_it() {
    a_master_that_names_five_thousand_pages_passes_the_budget();
}

#[test]
#[ignore = "timed, on a quiet machine: run with --ignored --nocapture"]
fn a_master_that_names_five_thousand_pages_passes_the_budget_is_timed() {
    let started = std::time::Instant::now();
    a_master_that_names_five_thousand_pages_passes_the_budget();
    let took = started.elapsed();
    eprintln!("took {took:?}");
    assert!(took < std::time::Duration::from_secs(60), "took {took:?}");
}
