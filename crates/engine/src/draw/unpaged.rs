//! The sheets no page works out: what a `sheet.*` command leaves stale on the document's sheet, a style's, a master's page sheet and the shapes of a master that no page holds
//! an instance of, and how it is put right.
//!
//! **Why.** A command's recalculation is by page ([`super::settle`]): the cells it wrote on the page's shapes and sheet, what reads them, and, for a change that is not on the page (the
//! document's sheet, a style, a master), every cell of every page whole. None of that works out a cell **of those sheets themselves**: a formula typed into the document's sheet kept the
//! 0 it was written with, the cells of the sheet that read it never followed it, and a `REF()` left by a deleted row had no `#REF!` recorded (the property test of found
//! each, by a full recalculation that disagreed with the drawing). A master's shape that a page holds an instance of is worked out by that page (its cells and its instances'), so only
//! one that no page holds is left: nothing reads the master but the master.
//!
//! **What is done.** [`work_out`] compares those sheets with the document as the call found it, cell by cell, and hands the cells that differ (written, added, rewritten by a renumber,
//! or taken away with their row or section) to `calc`'s own working out of what an edit changed ([`diacraft_calc::commit_changed`]): the cells are worked out, and so are the cells of
//! their sheet that read them, and only those are written, as for a page. It is done **before** the pages are worked out, so that a shape that names `TheDoc!User.Rate`
//! reads the value the formula gives. A sheet that holds no formula is left out (what was written to it is exact), and so is a call that changed none of these sheets: the comparison is
//! by reference (`Arc::ptr_eq`) wherever the call did not write, so it costs what the call wrote.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::Changes;
use diacraft_doc::{
    Cell, CellAddr, Cells, Document, FormulaSlot, MAX_SHAPE_DEPTH, Master, Owner, Row, RowKey, Section, ShapeList, ShapeRef, Sheet, SheetRef,
};

use super::Problem;
use super::sheet::pages_using_master;

/// Whether a sheet holds a formula of its own or one it takes from the sheet it is based on: a sheet that holds none has no cell a write can leave stale.
fn holds_formula(sheet: &Sheet) -> bool {
    let formula = |cell: &Cell| matches!(cell.formula, FormulaSlot::Local(_) | FormulaSlot::Inherit);
    sheet.cells.values().any(formula)
        || sheet.sections.iter().any(|s| s.cells.values().any(formula) || s.rows.iter().any(|r| r.cells.values().any(formula)))
}

/// What the cells of one place are now against what they were: the addresses of those that were written, added or taken away. `at` gives the address of a cell by its name.
fn cells_changed(old: &Cells, new: &Cells, at: &dyn Fn(&str) -> CellAddr, sheet: &SheetRef, out: &mut Vec<(SheetRef, CellAddr)>) {
    for (name, cell) in new {
        if old.get(name) != Some(cell) {
            out.push((sheet.clone(), at(name.as_str())));
        }
    }
    for name in old.keys().filter(|name| !new.contains_key(*name)) {
        out.push((sheet.clone(), at(name.as_str())));
    }
}

/// The cells of the rows of a section that differ: a row is found by its key, and a row that has another type or is deleted now counts every cell it holds.
fn rows_changed(old: &Section, new: &Section, sheet: &SheetRef, out: &mut Vec<(SheetRef, CellAddr)>) {
    let (name, ix) = (new.name.as_str(), new.ix);
    let was: HashMap<RowKey, &Arc<Row>> = old.rows.iter().filter_map(|r| Some((r.key(name)?, r))).collect();
    let mut now: HashSet<RowKey> = HashSet::new();
    let empty = Cells::new();
    for row in new.rows.iter() {
        let Some(key) = row.key(name) else { continue };
        let at = |cell: &str| CellAddr::in_row(name, ix, key.clone(), cell);
        match was.get(&key) {
            Some(old_row) if Arc::ptr_eq(old_row, row) => {}
            Some(old_row) if old_row.del != row.del || old_row.row_type != row.row_type => {
                cells_changed(&empty, &old_row.cells, &at, sheet, out);
                cells_changed(&empty, &row.cells, &at, sheet, out);
            }
            Some(old_row) => cells_changed(&old_row.cells, &row.cells, &at, sheet, out),
            None => cells_changed(&empty, &row.cells, &at, sheet, out),
        }
        now.insert(key);
    }
    for (key, old_row) in was.iter().filter(|(key, _)| !now.contains(*key)) {
        let at = |cell: &str| CellAddr::in_row(name, ix, (*key).clone(), cell);
        cells_changed(&old_row.cells, &empty, &at, sheet, out);
    }
}

/// The cells of one section that differ, those under the section and those in its rows.
fn section_changed(old: &Section, new: &Section, sheet: &SheetRef, out: &mut Vec<(SheetRef, CellAddr)>) {
    let (name, ix) = (new.name.as_str(), new.ix);
    cells_changed(&old.cells, &new.cells, &|cell| CellAddr::in_section(name, ix, cell), sheet, out);
    rows_changed(old, new, sheet, out);
}

/// The cells of a sheet that differ from what they were.
fn sheet_changed(old: &Sheet, new: &Sheet, sheet: &SheetRef, out: &mut Vec<(SheetRef, CellAddr)>) {
    cells_changed(&old.cells, &new.cells, &CellAddr::direct, sheet, out);
    let same = |a: &Section, b: &Section| a.name == b.name && a.ix == b.ix;
    for section in &new.sections {
        match old.sections.iter().find(|s| same(s, section)) {
            Some(was) if Arc::ptr_eq(was, section) => {}
            Some(was) => section_changed(was, section, sheet, out),
            None => section_changed(&Section::new(section.name.as_str(), section.ix), section, sheet, out),
        }
    }
    for gone in old.sections.iter().filter(|g| !new.sections.iter().any(|s| same(g, s))) {
        section_changed(gone, &Section::new(gone.name.as_str(), gone.ix), sheet, out);
    }
}

/// Notes the cells that differ between two copies of a sheet, when the sheet holds a formula and is not the same allocation.
fn note(old: &Arc<Sheet>, new: &Arc<Sheet>, sheet: &SheetRef, out: &mut Vec<(SheetRef, CellAddr)>) {
    if !Arc::ptr_eq(old, new) && holds_formula(new) {
        sheet_changed(old, new, sheet, out);
    }
}

/// The shapes of a master part, the old and the new list side by side by place: the cells that differ on those that are not the same allocation. A part whose shapes are not the same
/// in number is left alone: no `sheet.*` command adds or takes out a shape.
fn shapes_changed(owner: Owner, old: &ShapeList, new: &ShapeList, path: &mut Vec<u32>, out: &mut Vec<(SheetRef, CellAddr)>) {
    if old.len() != new.len() || path.len() >= MAX_SHAPE_DEPTH {
        return;
    }
    for (at, (was, now)) in old.iter().zip(new.iter()).enumerate() {
        let Ok(place) = u32::try_from(at) else { return };
        path.push(place);
        note(&was.sheet, &now.sheet, &SheetRef::Shape(ShapeRef { owner, path: path.clone() }), out);
        if !now.children.is_empty() {
            shapes_changed(owner, &was.children, &now.children, path, out);
        }
        path.pop();
    }
}

/// The cells of a master that differ: its page sheet, and its shapes when no page holds an instance of it.
fn master_changed(doc: &Document, old: &Master, new: &Master, out: &mut Vec<(SheetRef, CellAddr)>) {
    note(&old.sheet, &new.sheet, &SheetRef::Master(new.id), out);
    if old.shapes != new.shapes && pages_using_master(doc, new.id).is_empty() {
        shapes_changed(Owner::Master(new.id), &old.shapes, &new.shapes, &mut Vec::new(), out);
    }
}

/// The cells of the sheets no page works out that differ between `before` and `doc`, as sheet and address.
pub(super) fn changes(before: &Document, doc: &Document) -> Vec<(SheetRef, CellAddr)> {
    let mut out = Vec::new();
    note(&before.sheet, &doc.sheet, &SheetRef::Document, &mut out);
    for (old, new) in before.styles.iter().zip(doc.styles.iter()).filter(|(old, new)| old.id == new.id) {
        note(&old.sheet, &new.sheet, &SheetRef::Style(new.id), &mut out);
    }
    for (old, new) in before.masters.iter().zip(doc.masters.iter()).filter(|(old, new)| old.id == new.id) {
        master_changed(doc, old, new, &mut out);
    }
    out
}

/// Works out what a call left stale on the sheets no page works out ([`changes`]) and what reads it on those sheets, before the pages are worked out. A drawing with no page has none to
/// read them and nothing to do.
pub(super) fn work_out(doc: &mut Document, before: &Document) -> Result<(), Problem> {
    work_out_written(doc, before, &[])
}

/// [`work_out`] for a call that wrote `written` cells (as sheet and address, in the order of the call): **the cells of those that are on a master (its page sheet or its shapes) are worked out as
/// changed, whether or not the sheet holds a formula, when some sheet names a master** (`Masters[..]!`, [`diacraft_calc::masters_are_named`]): a number written to a master's shape that
/// no page holds an instance of is read by the pages that name the master, which no pass over the pages that hold an instance reaches. (The document's sheet and the styles are not asked: a write
/// to either works every page out whole, the cells that read it with them.) A drawing in which no sheet names a master costs the listing, which the next commit finds kept.
pub(super) fn work_out_written(doc: &mut Document, before: &Document, written: &[(SheetRef, CellAddr)]) -> Result<(), Problem> {
    let mut cells = changes(before, doc);
    let on_a_master =
        |sheet: &SheetRef| matches!(sheet, SheetRef::Master(_)) || matches!(sheet, SheetRef::Shape(at) if matches!(at.owner, Owner::Master(_)));
    if written.iter().any(|(sheet, _)| on_a_master(sheet)) && crate::calc_index::masters_are_named(doc) {
        let mut known: HashSet<(SheetRef, CellAddr)> = cells.iter().cloned().collect();
        for (sheet, addr) in written.iter().filter(|(sheet, _)| on_a_master(sheet)) {
            if known.insert((sheet.clone(), addr.clone())) {
                cells.push((sheet.clone(), addr.clone()));
            }
        }
    }
    let (Some(page), false) = (doc.pages.iter().next().map(|p| p.id), cells.is_empty()) else { return Ok(()) };
    let changes = Changes { cells, ..Changes::default() };
    // The cells of the pages that read these (`TheDoc!`, `Styles!`, `Masters[..]!`) are worked out in the same commit, on whatever page, and the connectors glued to their shapes are routed again as
    // after any command.
    super::settle::commit_reaching_all(doc, page, &changes).map_err(|problem| {
        if problem.bad_params || !problem.message.contains("too large to recalculate") {
            return problem;
        }
        Problem::failed("the sheet is too large to work out again after this edit: nothing was changed")
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::Value;
    use diacraft_testkit::build::{DocBuilder, MasterBuilder, PageBuilder, ShapeBuilder, StyleBuilder};

    use super::*;

    /// What the tests change: the master shape's `Width`, the style's `LineWeight`, and the document's `User.Rate`.
    struct Spec<'a> {
        width: &'a str,
        line: &'a str,
        rate: Option<(&'a str, &'a str)>,
        instance: bool,
    }

    fn drawing(c: &Spec<'_>) -> Document {
        let master = MasterBuilder::new().shape(ShapeBuilder::new(5).cell("Width", c.width).formula("Height", "Width*2", "4"));
        let style = StyleBuilder::new().name("S1").formula("LineWeight", c.line, "2");
        let page = if c.instance { PageBuilder::new().shape(ShapeBuilder::new(10).master(1)) } else { PageBuilder::new() };
        let mut doc = DocBuilder::new().master(1, master).style(1, style).page(1, page).build();
        let other = CellAddr::in_row("User", None, RowKey::name("Other"), "Value");
        Arc::make_mut(&mut doc.sheet).set_cell(&other, Cell::with_formula("2*2", Some(Value::from_text("4")))).unwrap();
        if let Some((text, formula)) = c.rate {
            let cell = Cell::with_formula(formula, Some(Value::from_text(text)));
            Arc::make_mut(&mut doc.sheet).set_cell(&rate(), cell).unwrap();
        }
        doc
    }

    fn plain() -> Spec<'static> {
        Spec { width: "2", line: "1+1", rate: Some(("2", "1+1")), instance: false }
    }

    fn rate() -> CellAddr {
        CellAddr::in_row("User", None, RowKey::name("Rate"), "Value")
    }

    #[test]
    fn a_call_that_wrote_nothing_on_these_sheets_leaves_nothing() {
        let before = drawing(&plain());
        assert_eq!(changes(&before, &before.clone()), []);
        assert_eq!(changes(&before, &drawing(&plain())), [], "two copies that are equal cell by cell, though not the same allocation");
    }

    #[test]
    fn a_sheet_that_holds_no_formula_is_left_out() {
        let mut before = DocBuilder::new().page(1, PageBuilder::new()).build();
        let mut after = before.clone();
        Arc::make_mut(&mut after.sheet).set_cell(&rate(), Cell::constant(Value::from_text("3"))).unwrap();
        assert_eq!(changes(&before, &after), [], "what was written to a sheet with no formula is exact, and nothing on it reads it");
        Arc::make_mut(&mut before.sheet).set_cell(&rate(), Cell::with_formula("1+1", Some(Value::from_text("2")))).unwrap();
        assert_eq!(changes(&before, &after), [], "the sheet is judged by what it holds now");
    }

    #[test]
    fn a_cell_written_or_taken_away_on_the_document_or_a_style_is_found() {
        let before = drawing(&plain());
        let written = drawing(&Spec { rate: Some(("5", "2+3")), ..plain() });
        assert_eq!(changes(&before, &written), [(SheetRef::Document, rate())]);
        let gone = drawing(&Spec { rate: None, ..plain() });
        assert_eq!(
            changes(&before, &gone),
            [(SheetRef::Document, rate())],
            "the cell of a row that went changed too: what reads it reads nothing now"
        );
        let styled = drawing(&Spec { line: "2+2", ..plain() });
        assert_eq!(changes(&before, &styled), [(SheetRef::Style(1), CellAddr::direct("LineWeight"))]);
    }

    #[test]
    fn the_shape_of_a_master_is_found_only_when_no_page_holds_an_instance_of_it() {
        let want = (SheetRef::Shape(ShapeRef { owner: Owner::Master(1), path: vec![0] }), CellAddr::direct("Width"));
        let changed = |instance: bool| changes(&drawing(&Spec { instance, ..plain() }), &drawing(&Spec { instance, width: "3", ..plain() }));
        assert_eq!(changed(false), [want]);
        assert_eq!(changed(true), [], "a page that holds an instance works the master's cell out, and this does not");
    }
}
