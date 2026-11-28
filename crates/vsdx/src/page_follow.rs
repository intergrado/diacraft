//! The formulas that name a page the writer renamed (`page_names.rs`).
//!
//! **A page the writer renames is named by DiaCraft and not by the person**, so no formula may be left reading `#REF!` for it: every formula that names such a page, by the name it had (the
//! `NameU` a file's formulas use, or its `Name`) or by a number (`Pages[2]`, a page by its `NameU`), is rewritten to name the page by its new name, and keeps its
//! value. A formula names a page by `Pages[<page>]!` before a reference or a call; the bytes between the brackets are replaced by the page's new universal name and every other
//! byte of the formula is kept ([`diacraft_formula::page_index_spans`]). The page a name finds is the first page of the list that has the name as its `NameU` or its `Name`, and a number finds
//! the first page whose `NameU` is it: as the evaluator finds it, against the pages as they were before the rename, so that a formula is rewritten only when it named the page that was renamed.
//! A formula that still finds that page by what it says (a `Name` the page keeps when only its `NameU` is new) is left as it is.
//!
//! The rewrite reaches every sheet that holds a formula: the document's, a style's, a master's and its shapes', a page's and its shapes' (members of groups included). A cell that inherits its
//! formula holds none of its own and is not touched: the sheet it inherits from is. Only a formula of the cell's own is rewritten, the cell's value, unit, error and every other field kept.
//!
//! **The links of a drawing follow the page too**. A link with no `Address` goes to a page of this drawing: its `SubAddress` is the page's name, or the page's name, a slash and a shape
//! A `SubAddress` that is plain text (a cell with no formula of its own) and names a page the writer renamed is written with the page's new name,
//! the blanks round it and the text after a slash kept; the page is found by its name as a click finds it, the first page with the name (and, after that, by its universal name, as the formulas do), against the pages as they
//! were. The `SubAddress` of a link with an address (the fragment of a web address) is not a page's name and is left. A `SubAddress` that is a formula is a formula like any other. An `.odg`'s own page links
//! are markup and follow in `page_hrefs.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellWrite, Document, FormulaSlot, MAX_SHAPE_DEPTH, Owner, Page, Shape, ShapeList, ShapeRef, Sheet, SheetRef, Value,
    WriteOrigin,
};
use diacraft_formula::{Index, page_index_spans};

/// Which page a name or a number finds in a list of pages, by position.
struct Finder {
    /// By `NameU` or `Name`: the first page that has it as either.
    named: HashMap<Arc<str>, usize>,
    /// By `NameU` alone.
    universal: HashMap<Arc<str>, usize>,
}

impl Finder {
    fn of<'p>(pages: impl Iterator<Item = &'p Arc<Page>>) -> Finder {
        let mut named: HashMap<Arc<str>, usize> = HashMap::new();
        let mut universal: HashMap<Arc<str>, usize> = HashMap::new();
        for (at, page) in pages.enumerate() {
            if let Some(u) = &page.name_u {
                named.entry(Arc::clone(u)).or_insert(at);
                universal.entry(Arc::clone(u)).or_insert(at);
            }
            named.entry(Arc::clone(&page.name)).or_insert(at);
        }
        Finder { named, universal }
    }

    /// The position of the page `index` finds.
    fn find(&self, index: &Index) -> Option<usize> {
        match index {
            Index::Name(name) => self.named.get(&**name).copied(),
            Index::Number(n) => self.universal.get(n.to_string().as_str()).copied(),
        }
    }
}

/// The pages of a list by the name a link names them by, and the names they have now, by position (the two lists are in the same order).
struct Targets {
    /// By `Name`: the first page that has it.
    by_name: HashMap<Arc<str>, usize>,
    /// By `NameU`: the first page that has it.
    by_universal: HashMap<Arc<str>, usize>,
    /// The `Name` and `NameU` each page has now.
    now: Vec<(Arc<str>, Option<Arc<str>>)>,
    /// The `Name` and `NameU` each page had.
    was: Vec<(Arc<str>, Option<Arc<str>>)>,
}

impl Targets {
    fn of(was: &Document, now: &Document) -> Targets {
        let (mut by_name, mut by_universal) = (HashMap::new(), HashMap::new());
        let names =
            |doc: &Document| -> Vec<(Arc<str>, Option<Arc<str>>)> { doc.pages.iter().map(|p| (Arc::clone(&p.name), p.name_u.clone())).collect() };
        let was_names = names(was);
        for (at, (name, universal)) in was_names.iter().enumerate() {
            by_name.entry(Arc::clone(name)).or_insert(at);
            if let Some(u) = universal {
                by_universal.entry(Arc::clone(u)).or_insert(at);
            }
        }
        Targets { by_name, by_universal, now: names(now), was: was_names }
    }

    /// The new name of the page that `name` names as the link of a drawing does (by its `Name`, else by its `NameU`); `None` when no page is named so, or the page has the name still.
    fn new_name(&self, name: &str) -> Option<Arc<str>> {
        if let Some(&at) = self.by_name.get(name) {
            let (was, now) = (self.was.get(at)?, self.now.get(at)?);
            return (was.0 != now.0).then(|| Arc::clone(&now.0));
        }
        let at = *self.by_universal.get(name)?;
        let (was, now) = (self.was.get(at)?.1.as_ref()?, self.now.get(at)?.1.as_ref()?);
        (was != now).then(|| Arc::clone(now))
    }

    /// `text`, the `SubAddress` of a link of this drawing, with the page it names given its new name; `None` when it names no page that was renamed. The page is the whole text, else the text up
    /// to a slash (the first that names a page: a page's name may hold a slash), as a click finds it, and the blanks round the text stay.
    fn followed(&self, text: &str) -> Option<String> {
        let lead = text.len() - text.trim_start().len();
        let body = text.trim();
        if body.is_empty() {
            return None;
        }
        let ends = std::iter::once(body.len()).chain(body.match_indices('/').map(|(at, _)| at).take(64));
        for end in ends {
            let name = body.get(..end)?;
            let found = self.by_name.contains_key(name) || self.by_universal.contains_key(name);
            if !found {
                continue;
            }
            let new = self.new_name(name)?;
            let mut out = String::with_capacity(text.len() + new.len());
            out.push_str(text.get(..lead)?);
            out.push_str(&new);
            out.push_str(text.get(lead + end..)?);
            return Some(out);
        }
        None
    }
}

/// `source` with the index of each qualifier that named a renamed page replaced by the page's new name; `None` when no index is.
fn followed(source: &str, was: &Finder, now: &Finder, renamed: &HashMap<usize, Arc<str>>) -> Option<String> {
    let mut out = String::new();
    // The text before `copied` is in `out`.
    let mut copied = 0_usize;
    for span in page_index_spans(source) {
        let Some(at) = was.find(&span.index) else { continue };
        let Some(new_name) = renamed.get(&at) else { continue };
        // A name the page still answers to stands.
        if now.find(&span.index) == Some(at) {
            continue;
        }
        out.push_str(source.get(copied..span.inside.start)?);
        out.push_str(new_name);
        copied = span.inside.end;
    }
    if copied == 0 {
        return None;
    }
    out.push_str(source.get(copied..)?);
    Some(out)
}

/// Whether a formula's text holds a qualifier that may name a page: the word `Pages` in any case and a bracket. A cheap test, so that a cell is looked at only when it may need the work.
fn mentions_pages(source: &str) -> bool {
    source.as_bytes().windows(6).any(|w| w.get(..5).is_some_and(|word| word.eq_ignore_ascii_case(b"pages")) && w.get(5) == Some(&b'['))
}

/// The cells of `sheet` whose formula is their own and may name a page, with their addresses: those directly under it, under each section, and in each row.
fn candidates(sheet: &Sheet) -> Vec<(CellAddr, &Cell)> {
    let may = |cell: &Cell| matches!(&cell.formula, FormulaSlot::Local(f) if mentions_pages(&f.source));
    let mut out: Vec<(CellAddr, &Cell)> = Vec::new();
    for (name, cell) in &sheet.cells {
        if may(cell) {
            out.push((CellAddr::direct(name.as_str()), cell));
        }
    }
    for section in &sheet.sections {
        for (name, cell) in &section.cells {
            if may(cell) {
                out.push((CellAddr::in_section(section.name.as_str(), section.ix, name.as_str()), cell));
            }
        }
        for row in &section.rows {
            let Some(key) = row.key(section.name.as_str()) else { continue };
            for (name, cell) in &row.cells {
                if may(cell) {
                    out.push((CellAddr::in_row(section.name.as_str(), section.ix, key.clone(), name.as_str()), cell));
                }
            }
        }
    }
    out
}

/// The text a cell holds as a value, when it has no formula of its own.
fn plain_text(cell: &Cell) -> Option<&str> {
    if !matches!(cell.formula, FormulaSlot::Absent) {
        return None;
    }
    cell.value.as_ref().map(Value::text)
}

/// The `SubAddress` cells of `sheet`'s `Hyperlink` rows that name a renamed page, rewritten, with their addresses: a link with no `Address` (a link of this drawing), a `SubAddress` that is plain text.
fn link_cells(sheet: &Sheet, targets: &Targets) -> Vec<(CellAddr, Cell)> {
    let mut out: Vec<(CellAddr, Cell)> = Vec::new();
    for section in sheet.sections.iter().filter(|section| section.name.as_str() == "Hyperlink" && !section.del) {
        for row in section.rows.iter().filter(|row| !row.del) {
            let Some(key) = row.key(section.name.as_str()) else { continue };
            let Some(cell) = row.cells.get("SubAddress") else { continue };
            // The row goes to a place in this drawing when its `Address` says nothing: no cell, or a constant that is blank.
            let outside = row
                .cells
                .get("Address")
                .is_some_and(|a| !matches!(a.formula, FormulaSlot::Absent) || plain_text(a).is_some_and(|t| !t.trim().is_empty()));
            if outside {
                continue;
            }
            let Some(new) = plain_text(cell).and_then(|text| targets.followed(text)) else { continue };
            let next = Cell { value: Some(Value::from_text(&new)), ..cell.clone() };
            out.push((CellAddr::in_row(section.name.as_str(), section.ix, key, "SubAddress"), next));
        }
    }
    out
}

/// What a walk of the sheets finds to write: the formulas that follow a page and the links that do.
#[derive(Default)]
struct Found {
    formulas: Vec<(SheetRef, CellWrite)>,
    links: Vec<(SheetRef, CellWrite)>,
}

/// The pages the writer renamed, as the walk of a sheet needs them.
struct Renames<'a> {
    before: &'a Finder,
    after: &'a Finder,
    renamed: &'a HashMap<usize, Arc<str>>,
    targets: &'a Targets,
}

/// The writes that make the formulas and the links of `sheet` follow the pages that were renamed, appended to `found`.
fn follow_sheet(at: &dyn Fn() -> SheetRef, sheet: &Sheet, renames: &Renames<'_>, found: &mut Found) {
    let mut place: Option<SheetRef> = None;
    for (addr, cell) in candidates(sheet) {
        let FormulaSlot::Local(formula) = &cell.formula else { continue };
        let Some(source) = followed(&formula.source, renames.before, renames.after, renames.renamed) else { continue };
        let rewritten = diacraft_formula::parse(&source);
        // A formula that read is one that reads still: a rewrite that would not is left out.
        if formula.ast.is_ok() && rewritten.ast.is_err() {
            continue;
        }
        let next = Cell { formula: FormulaSlot::Local(Arc::new(rewritten)), ..cell.clone() };
        let sheet_ref = place.get_or_insert_with(at).clone();
        found.formulas.push((sheet_ref, CellWrite { addr, edit: CellEdit::Cell(next), origin: WriteOrigin::AutomationForce }));
    }
    for (addr, next) in link_cells(sheet, renames.targets) {
        let sheet_ref = place.get_or_insert_with(at).clone();
        found.links.push((sheet_ref, CellWrite { addr, edit: CellEdit::Cell(next), origin: WriteOrigin::AutomationForce }));
    }
}

/// Walks the shapes of `list` depth first, without recursion and no deeper than the model allows, giving each one's path.
pub(crate) fn each_shape<'l>(list: &'l ShapeList, visit: impl FnMut(&[u32], &'l Shape)) {
    each_shape_under(list, &[], visit);
}

/// [`each_shape`] for the shapes of `list` that are the members of the shape at `prefix`: every path starts with it, and the walk goes no deeper than the model allows for the whole path.
pub(crate) fn each_shape_under<'l>(list: &'l ShapeList, prefix: &[u32], mut visit: impl FnMut(&[u32], &'l Shape)) {
    let mut stack: Vec<(&ShapeList, usize)> = vec![(list, 0)];
    let mut path: Vec<u32> = prefix.to_vec();
    while let Some(frame) = stack.last_mut() {
        let (items, at) = *frame;
        let Some(shape) = items.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        path.truncate(prefix.len().saturating_add(stack.len()).saturating_sub(1));
        path.push(u32::try_from(at).unwrap_or(u32::MAX));
        visit(&path, shape.as_ref());
        if prefix.len().saturating_add(stack.len()) < MAX_SHAPE_DEPTH {
            stack.push((&shape.children, 0));
        }
    }
}

/// What a rewrite of the names a drawing's formulas and links give pages did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Followed {
    /// How many formulas were rewritten.
    pub formulas: usize,
    /// How many links, the `SubAddress` of a `Hyperlink` row, were.
    pub links: usize,
}

/// Writes the cells, in one batch (all or nothing); when the document refuses the batch (a cell in a row that is deleted here), each alone, the ones it refuses left out. → how many were written.
fn write_all(now: &mut Document, writes: Vec<(SheetRef, CellWrite)>) -> usize {
    let count = writes.len();
    if count == 0 || now.write_batch(writes.clone()).is_ok() {
        return count;
    }
    writes.into_iter().filter(|(sheet, write)| now.write_cells(sheet, vec![write.clone()]).is_ok()).count()
}

/// Makes the formulas and the links of `now` that named a page of `was` that was renamed name it by its new name. `renamed` holds the new universal name of each page renamed, by its position in the
/// list (the list is as long in both, and in the same order).
pub(crate) fn follow_renames(now: &mut Document, was: &Document, renamed: &HashMap<usize, Arc<str>>) -> Followed {
    if renamed.is_empty() {
        return Followed::default();
    }
    let (before, after) = (Finder::of(was.pages.iter()), Finder::of(now.pages.iter()));
    let targets = Targets::of(was, now);
    let renames = Renames { before: &before, after: &after, renamed, targets: &targets };
    let mut found = Found::default();
    follow_sheet(&|| SheetRef::Document, &now.sheet, &renames, &mut found);
    for style in now.styles.iter() {
        follow_sheet(&|| SheetRef::Style(style.id), &style.sheet, &renames, &mut found);
    }
    for master in now.masters.iter() {
        follow_sheet(&|| SheetRef::Master(master.id), &master.sheet, &renames, &mut found);
        each_shape(&master.shapes, |path, shape| {
            follow_sheet(&|| SheetRef::Shape(ShapeRef { owner: Owner::Master(master.id), path: path.to_vec() }), &shape.sheet, &renames, &mut found);
        });
    }
    for page in now.pages.iter() {
        follow_sheet(&|| SheetRef::Page(page.id), &page.sheet, &renames, &mut found);
        each_shape(&page.shapes, |path, shape| {
            follow_sheet(&|| SheetRef::Shape(ShapeRef { owner: Owner::Page(page.id), path: path.to_vec() }), &shape.sheet, &renames, &mut found);
        });
    }
    let Found { formulas, links } = found;
    Followed { formulas: write_all(now, formulas), links: write_all(now, links) }
}
