//! The cells that read the sheets no page holds, and what a style's formula gives a shape (the last of gap F40): a cell of the document's sheet, of a style or of a master that a
//! commit worked out is **read by the cells of every page that name it** (`TheDoc!`, `Styles!`, `Masters[..]!`), in the one commit; a formula a shape **inherits from a style** is listed and
//! classified for the shape as one it inherits from a master is. Each case is held to what no change may break: a full evaluation changes no cell after the commit.

use std::sync::Arc;

use diacraft_calc::{Budget, Changes, CommitReport, EvalOptions, ReaderIndex, changes_between_with, commit_changed_across, recalc_all};
use diacraft_doc::{Cell, CellAddr, CellEdit, Document, Lookup, Owner, RowKey, ShapeRef, SheetRef, Value as DocValue, WriteOrigin};
use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

fn width() -> CellAddr {
    CellAddr::direct("Width")
}

fn height() -> CellAddr {
    CellAddr::direct("Height")
}

fn total() -> CellAddr {
    CellAddr::in_row("User", None, RowKey::name("Total"), "Value")
}

fn line_weight() -> CellAddr {
    CellAddr::direct("LineWeight")
}

fn top(page: u32, place: u32) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Page(page), place))
}

/// The value a cell of a sheet holds, and the error it holds.
fn held_in(doc: &Document, sheet: &SheetRef, addr: &CellAddr) -> (Option<String>, Option<String>) {
    let found = match sheet {
        SheetRef::Document => doc.sheet.lookup(addr),
        SheetRef::Style(id) => match doc.styles.iter().find(|s| s.id == *id) {
            Some(style) => style.sheet.lookup(addr),
            None => return (None, None),
        },
        SheetRef::Shape(at) => match doc.shape(at) {
            Some(shape) => shape.sheet.lookup(addr),
            None => return (None, None),
        },
        _ => return (None, None),
    };
    match found {
        Lookup::Found(cell) => (cell.value.as_ref().map(|v| v.text().to_owned()), cell.error.as_ref().map(|e| e.to_string())),
        _ => (None, None),
    }
}

fn value_in(doc: &Document, sheet: &SheetRef, addr: &CellAddr) -> Option<String> {
    held_in(doc, sheet, addr).0
}

fn agrees_with_a_full_evaluation(doc: &Document) {
    let report = recalc_all(doc, &Budget::default());
    assert!(report.differed.is_empty() && !report.budget_exhausted, "{:?}", report.differed);
}

/// Writes the width of shape 3 of page 1 and finds what changed, as an editing command does.
fn write_plan_width(doc: &mut Document, text: &str) -> Changes {
    let before = doc.clone();
    doc.write_cell(&top(1, 0), &width(), CellEdit::Value(DocValue::from_text(text)), WriteOrigin::Drawing).expect("a write");
    changes_between_with(&mut ReaderIndex::new(), &before, doc, 1)
}

fn commit(index: &mut ReaderIndex, doc: &mut Document, page: u32, changes: &Changes) -> CommitReport {
    commit_changed_across(index, doc, &Budget::default(), EvalOptions::default(), page, changes)
}

fn set_cell(doc: &mut Document, sheet: &SheetRef, addr: &CellAddr, cell: Cell) {
    doc.write_cell(sheet, addr, CellEdit::Cell(cell), WriteOrigin::Automation).expect("a write");
}

/// A formula cell with the value it was saved with.
fn formula(text: &str, saved: &str) -> Cell {
    Cell::with_formula(text, Some(DocValue::from_text(saved)))
}

// ---- the document's sheet ---------------------------------------------------------------------------------------------------------------------

/// Page "Plan" holds shape 3 of width 5. The document's `User.Total` is three times it. Pages 2 and 3 each hold a shape whose height reads `TheDoc!User.Total`.
fn plan_total_and_two_readers() -> Document {
    let mut doc = DocBuilder::new()
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+1", "16")))
        .page(3, PageBuilder::new().name("Other").shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+2", "17")))
        .build();
    Arc::make_mut(&mut doc.sheet).set_cell(&total(), formula("Pages[Plan]!Sheet.3!Width*3", "15")).expect("a cell");
    doc
}

#[test]
fn a_shape_of_another_page_that_reads_a_document_cell_a_page_cell_changed_follows_it() {
    let mut doc = plan_total_and_two_readers();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Document, &total()).as_deref(), Some("21"), "the document's cell: {report:?}");
    assert_eq!(value_in(&doc, &top(2, 0), &height()).as_deref(), Some("22"), "the shape of page 2: {report:?}");
    assert_eq!(value_in(&doc, &top(3, 0), &height()).as_deref(), Some("23"), "the shape of page 3: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// The document's cell that is read is on the sheet no page holds, and the cell that reads it is one of a style, which no page holds either.
#[test]
fn a_style_cell_that_reads_a_document_cell_a_page_cell_changed_follows_it() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "TheDoc!User.Total+1", "16");
    let mut doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Other"))
        .build();
    Arc::make_mut(&mut doc.sheet).set_cell(&total(), formula("Pages[Plan]!Sheet.3!Width*3", "15")).expect("a cell");
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Style(1), &line_weight()).as_deref(), Some("22"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

// ---- a style's cell, a master's cell ------------------------------------------------------------------------------------------------------------

#[test]
fn a_shape_of_another_page_that_reads_a_style_cell_a_page_cell_changed_follows_it() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let mut doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "Styles!Rated!LineWeight+1", "11")))
        .build();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Style(1), &line_weight()).as_deref(), Some("14"), "the style's cell: {report:?}");
    assert_eq!(value_in(&doc, &top(2, 0), &height()).as_deref(), Some("15"), "the shape that reads it: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A master with a shape whose width reads a page; a shape of page 2, with no instance of the master, reads the master's shape by its name.
#[test]
fn a_shape_of_another_page_that_reads_a_master_cell_a_page_cell_changed_follows_it() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Reader").shape(ShapeBuilder::new(10).formula("Height", "Masters[Meter]!Sheet.5!Width+1", "11")))
        .build();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    let master_shape = SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0));
    assert_eq!(value_in(&doc, &master_shape, &width()).as_deref(), Some("14"), "the master's cell: {report:?}");
    assert_eq!(value_in(&doc, &top(2, 0), &height()).as_deref(), Some("15"), "the shape that reads it: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

// ---- a formula a shape inherits from a style --------------------------------------------------------------------------------------------------

/// Style 1 holds a line weight that reads the page "Plan"; shape 10 of page 2 is linked to it (its line style) and holds the cell as `Inh`, with the value it was saved with.
fn style_that_reads_a_page() -> Document {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).links(Some(1), None, None).inh("LineWeight", Some("10"))))
        .build()
}

#[test]
fn a_shape_that_inherits_a_style_formula_that_names_a_page_follows_a_write_to_it() {
    let mut doc = style_that_reads_a_page();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(
        value_in(&doc, &top(2, 0), &line_weight()).as_deref(),
        Some("14"),
        "the shape reads the page through the formula it inherits: {report:?}"
    );
    assert_eq!(value_in(&doc, &SheetRef::Style(1), &line_weight()).as_deref(), Some("14"), "and so does the style's own cell: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

// ---- what the cells that were reached pass on, and what an instance inherits -----------------------------------------------------------------

/// A cell of another page that the document's cell reached passes the change on through its own page's graph: shape 11 of page 3 reads shape 10 of page 3.
#[test]
fn a_cell_that_a_document_cell_reached_passes_the_change_on_to_the_readers_on_its_own_page() {
    let mut doc = plan_total_and_two_readers();
    let page_3 = doc.pages.iter().position(|p| p.id == 3).expect("page 3");
    let follower = ShapeBuilder::new(11).formula("Width", "Sheet.10!Height*2", "34");
    let rebuilt = PageBuilder::new().name("Other").shape(ShapeBuilder::new(10).formula("Height", "TheDoc!User.Total+2", "17")).shape(follower);
    let mut page = (**doc.pages.get(page_3).expect("page 3")).clone();
    page.shapes = DocBuilder::new().page(3, rebuilt).build().pages.get(0).expect("a page").shapes.clone();
    doc.pages.set(page_3, Arc::new(page)).expect("page 3");
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(3, 0), &height()).as_deref(), Some("23"), "{report:?}");
    assert_eq!(value_in(&doc, &top(3, 1), &width()).as_deref(), Some("46"), "the shape that reads the one that was reached: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A cell of the document's sheet that reads another cell of it **through the qualifier**, and a style's cell that reads one of the document's.
#[test]
fn a_document_cell_that_names_the_document_by_its_qualifier_follows_the_cell_it_reads() {
    let mut doc = plan_total_and_two_readers();
    let twice = CellAddr::in_row("User", None, RowKey::name("Twice"), "Value");
    Arc::make_mut(&mut doc.sheet).set_cell(&twice, formula("TheDoc!User.Total*2", "30")).expect("a cell");
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Document, &twice).as_deref(), Some("42"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// An instance on a page that holds no formula of its own inherits a formula that reads the document's cell: it follows the cell, and so does the master's own.
#[test]
fn an_instance_that_inherits_a_formula_that_reads_a_document_cell_follows_it() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "TheDoc!User.Total*2", "30"));
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).master(1).inh("Width", Some("30"))))
        .build();
    Arc::make_mut(&mut doc.sheet).set_cell(&total(), formula("Pages[Plan]!Sheet.3!Width*3", "15")).expect("a cell");
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(2, 0), &width()).as_deref(), Some("42"), "the instance: {report:?}");
    let master_shape = SheetRef::Shape(ShapeRef::top(Owner::Master(1), 0));
    assert_eq!(value_in(&doc, &master_shape, &width()).as_deref(), Some("42"), "the master's cell: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// An instance that holds no `Width` of its own reads the master's through inheritance, and its own `Height` reads that: when the master's cell changes (it reads a page) the instance's
/// cell that reads it follows.
#[test]
fn an_instance_cell_that_reads_a_master_cell_it_inherits_follows_it() {
    let master = MasterBuilder::new().name("Meter").shape(ShapeBuilder::new(5).formula("Width", "Pages[Plan]!Sheet.3!Width*2", "10"));
    let mut doc = DocBuilder::new()
        .master(1, master)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).master(1).formula("Height", "Width+1", "11")))
        .build();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(2, 0), &height()).as_deref(), Some("15"), "{report:?}");
    agrees_with_a_full_evaluation(&doc);
}

/// A shape that inherits a style's formula that reads a shape of its own page: the graph of the page lists the cell for the shape (the drawing has one page).
#[test]
fn a_shape_that_inherits_a_style_formula_that_reads_a_shape_of_its_page_follows_it() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Sheet.3!Width*2", "10");
    let mut doc = DocBuilder::new()
        .style(1, rated)
        .page(
            1,
            PageBuilder::new()
                .name("Plan")
                .shape(ShapeBuilder::new(3).cell("Width", "5"))
                .shape(ShapeBuilder::new(10).links(Some(1), None, None).inh("LineWeight", Some("10"))),
        )
        .build();
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(1, 1), &line_weight()).as_deref(), Some("14"), "{report:?}");
    // (No full evaluation here: the style's own cell reads `Sheet.3` on the style, which holds no such shape, and gives `#REF!` there; the formula is meant for the shapes that inherit it.)
}

/// A shape linked to a style whose formula names no page is left alone, and one linked to a style that names one is not asked for its cells unless a link names it.
#[test]
fn a_shape_whose_links_name_no_style_that_holds_such_a_formula_is_not_worked_out() {
    let mut doc = style_that_reads_a_page();
    // Style 2 holds no formula, and the shape is linked to it.
    let plain = StyleBuilder::new().name("Plain").cell("LineWeight", "3");
    let mut rebuilt = DocBuilder::new()
        .style(1, StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10"))
        .style(2, plain)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).links(Some(2), None, None).inh("LineWeight", Some("10"))))
        .build();
    std::mem::swap(&mut doc, &mut rebuilt);
    let changes = write_plan_width(&mut doc, "7");
    commit(&mut ReaderIndex::new(), &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(2, 0), &line_weight()).as_deref(), Some("10"), "the style it is linked to holds no formula that names a page");
}

/// A shape's links are what its inherited formulas are found by: re-linked to the style that names a page, the shape is read from then on, and a style edited to name a page is too.
#[test]
fn a_shape_linked_to_another_style_and_a_style_edited_to_name_a_page_are_followed_from_then_on() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let plain = StyleBuilder::new().name("Plain").formula("LineWeight", "2+3", "5");
    let mut doc = DocBuilder::new()
        .style(1, rated)
        .style(2, plain)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(ShapeBuilder::new(10).links(Some(2), None, None).inh("LineWeight", Some("5"))))
        .build();
    let mut index = ReaderIndex::new();
    let changes = write_plan_width(&mut doc, "6");
    commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(2, 0), &line_weight()).as_deref(), Some("5"), "the style it is linked to names no page");
    // The shape is linked to the style that does.
    {
        let at = doc.pages.iter().position(|p| p.id == 2).expect("page 2");
        let mut page = (**doc.pages.get(at).expect("page 2")).clone();
        let mut shape = (**page.shapes.get(0).expect("a shape")).clone();
        let mut sheet = (*shape.sheet).clone();
        sheet.links.line = Some(1);
        shape.sheet = Arc::new(sheet);
        page.shapes.set(0, Arc::new(shape)).expect("a shape");
        doc.pages.set(at, Arc::new(page)).expect("page 2");
    }
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(
        value_in(&doc, &top(2, 0), &line_weight()).as_deref(),
        Some("14"),
        "the shape is linked to the style that reads the page now: {report:?}"
    );
    agrees_with_a_full_evaluation(&doc);
    // The style it was linked to is edited to name the page too, and the shape is linked back to it.
    set_cell(&mut doc, &SheetRef::Style(2), &line_weight(), formula("Pages[Plan]!Sheet.3!Width*3", "5"));
    {
        let at = doc.pages.iter().position(|p| p.id == 2).expect("page 2");
        let mut page = (**doc.pages.get(at).expect("page 2")).clone();
        let mut shape = (**page.shapes.get(0).expect("a shape")).clone();
        let mut sheet = (*shape.sheet).clone();
        sheet.links.line = Some(2);
        shape.sheet = Arc::new(sheet);
        page.shapes.set(0, Arc::new(shape)).expect("a shape");
        doc.pages.set(at, Arc::new(page)).expect("page 2");
    }
    let changes = write_plan_width(&mut doc, "8");
    let report = commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &top(2, 0), &line_weight()).as_deref(), Some("24"), "the edited style names the page now: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

fn rename(doc: &mut Document, id: u32, name: &str) {
    let at = doc.pages.iter().position(|p| p.id == id).expect("the page");
    Arc::make_mut(doc.pages.get_mut(at).expect("the page")).name = Arc::from(name);
}

/// A page renamed is a name the formula a shape inherits from a style does not find either: the shape's cell and the style's own are worked out.
#[test]
fn a_shape_that_inherits_a_style_formula_that_names_a_renamed_page_reads_nothing() {
    let mut before = style_that_reads_a_page();
    Arc::make_mut(before.pages.get_mut(0).expect("a page")).name_u = Some(Arc::from("Site-1"));
    let mut doc = before.clone();
    rename(&mut doc, 1, "Dach");
    let changes = Changes { page_names: diacraft_calc::PageNames::between(&before, &doc), ..Changes::default() };
    let report = commit_changed_across(&mut ReaderIndex::new(), &mut doc, &Budget::default(), EvalOptions::default(), 1, &changes);
    assert_eq!(held_in(&doc, &top(2, 0), &line_weight()).1.as_deref(), Some("#REF!"), "the shape: {report:?}");
    assert_eq!(held_in(&doc, &SheetRef::Style(1), &line_weight()).1.as_deref(), Some("#REF!"), "the style: {report:?}");
    agrees_with_a_full_evaluation(&doc);
}

// ---- the sheets that name the document's sheet or a style are listed only when a chain can change them ---------------------------------------------

/// Three pages of fifty shapes each whose heights read `TheDoc!User.Rate`, and a page "Plan" (page 1) with shape 3. The document's `User.Rate` is a number.
fn many_readers_of_a_number() -> Document {
    let mut b = DocBuilder::new().page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")));
    for p in 2..=4 {
        let mut page = PageBuilder::new().name(&format!("P{p}"));
        for k in 0..50 {
            page = page.shape(ShapeBuilder::new(10 + k).formula("Height", "TheDoc!User.Rate.Value*2", "6"));
        }
        b = b.page(p, page);
    }
    let mut doc = b.build();
    let rate = CellAddr::in_row("User", None, RowKey::name("Rate"), "Value");
    Arc::make_mut(&mut doc.sheet).set_cell(&rate, Cell::constant(DocValue::from_text("3"))).expect("a cell");
    doc
}

/// A document whose sheet holds a number, and styles that hold numbers, has no cell a chain from a page changes: the 150 shapes that read it are **not listed**, and cost nothing to the commit
/// (their text is not even walked). When the document's cell is made to read a page, the listing holds them from the next commit on, and a write to the page reaches every one.
#[test]
fn the_shapes_that_name_the_document_are_listed_only_once_the_document_reads_beyond_itself() {
    let mut doc = many_readers_of_a_number();
    let mut index = ReaderIndex::new();
    let changes = write_plan_width(&mut doc, "6");
    let closed = commit(&mut index, &mut doc, 1, &changes);
    let held_closed = index.held_bytes();
    assert_eq!(value_in(&doc, &top(2, 0), &height()).as_deref(), Some("6"), "nothing changed what they read: {closed:?}");
    // The document's cell reads the page now.
    let rate = CellAddr::in_row("User", None, RowKey::name("Rate"), "Value");
    set_cell(&mut doc, &SheetRef::Document, &rate, formula("Pages[Plan]!Sheet.3!Width*3", "18"));
    let changes = write_plan_width(&mut doc, "7");
    let open = commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Document, &rate).as_deref(), Some("21"), "{open:?}");
    assert_eq!(value_in(&doc, &top(4, 49), &height()).as_deref(), Some("42"), "every shape reads the document's cell now: {open:?}");
    let held_open = index.held_bytes();
    assert!(held_open > held_closed + 150 * 100, "the listing holds the shapes now: {held_closed} bytes then, {held_open} now");
    assert!(
        open.stats.steps > closed.stats.steps + 150,
        "and so does the commit that works them out: {} steps then, {} now",
        closed.stats.steps,
        open.stats.steps
    );
    agrees_with_a_full_evaluation(&doc);
}

/// The same for the styles: a style that holds numbers opens nothing, and one that reads a page does.
#[test]
fn the_shapes_that_name_a_style_are_listed_only_once_a_style_reads_beyond_itself() {
    let rated = StyleBuilder::new().name("Rated").cell("LineWeight", "3");
    let mut b = DocBuilder::new().style(1, rated).page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")));
    for k in 0..60 {
        b = b.page(2 + k, PageBuilder::new().shape(ShapeBuilder::new(10).formula("Height", "Styles!Rated!LineWeight*2", "6")));
    }
    let mut doc = b.build();
    let mut index = ReaderIndex::new();
    let changes = write_plan_width(&mut doc, "6");
    commit(&mut index, &mut doc, 1, &changes);
    let held_closed = index.held_bytes();
    set_cell(&mut doc, &SheetRef::Style(1), &line_weight(), formula("Pages[Plan]!Sheet.3!Width*2", "12"));
    let changes = write_plan_width(&mut doc, "7");
    let report = commit(&mut index, &mut doc, 1, &changes);
    assert_eq!(value_in(&doc, &SheetRef::Style(1), &line_weight()).as_deref(), Some("14"), "{report:?}");
    assert_eq!(value_in(&doc, &top(61, 0), &height()).as_deref(), Some("28"), "{report:?}");
    assert!(index.held_bytes() > held_closed + 60 * 100, "the listing holds the shapes now: {held_closed} then, {} now", index.held_bytes());
    agrees_with_a_full_evaluation(&doc);
}

// ---- a shape with a great many inherited cells ----------------------------------------------------------------------------------------------------

/// A shape linked to a style that reads a page, with twenty thousand cells it holds as `Inh`: each is asked of the chain, **a step each**, and a commit with fewer steps than that is stopped at its
/// budget in the words of any commit that passes one (nothing is written), where one with enough works out the cell that reads the page.
#[test]
fn the_inherited_cells_of_a_shape_are_charged_a_step_each_and_a_commit_is_stopped_at_its_budget() {
    let rated = StyleBuilder::new().name("Rated").formula("LineWeight", "Pages[Plan]!Sheet.3!Width*2", "10");
    let mut shape = ShapeBuilder::new(10).links(Some(1), None, None).inh("LineWeight", Some("10"));
    for k in 0..20_000 {
        shape = shape.row_inh("Geometry", Some(0), RowKey::Ix(k), "X", Some("1"));
    }
    let doc = DocBuilder::new()
        .style(1, rated)
        .page(1, PageBuilder::new().name("Plan").shape(ShapeBuilder::new(3).cell("Width", "5")))
        .page(2, PageBuilder::new().name("Board").shape(shape))
        .build();
    let mut small = doc.clone();
    let changes = write_plan_width(&mut small, "7");
    let report = commit_changed_across(&mut ReaderIndex::new(), &mut small, &Budget::with_steps(10_000), EvalOptions::default(), 1, &changes);
    assert!(report.budget_exhausted, "{report:?}");
    assert!(report.stats.steps <= 10_000 + 64, "the steps stop at the budget: {report:?}");
    assert_eq!(value_in(&small, &top(2, 0), &line_weight()).as_deref(), Some("10"), "the shape's cell is as it was");
    let mut whole = doc;
    let changes = write_plan_width(&mut whole, "7");
    let report = commit(&mut ReaderIndex::new(), &mut whole, 1, &changes);
    assert!(!report.budget_exhausted, "{report:?}");
    assert!(report.stats.steps >= 20_000, "a step for each cell asked: {report:?}");
    assert_eq!(value_in(&whole, &top(2, 0), &line_weight()).as_deref(), Some("14"), "{report:?}");
}
