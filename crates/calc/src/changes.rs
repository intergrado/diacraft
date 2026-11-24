//! What an edit changed: the cells that differ between a document and the one the edit started from.
//!
//! An editing command works out the page it edited ([`crate::commit_changed`]). It must not write the cached value of a cell it did not touch
//! and nothing that reads one it touched: on a file from another application a cell's saved value is the other application's, and where
//! this evaluator would give another result, working it out and writing it would change a cell nobody edited. So the command says what it
//! changed, and what it changed is found here by comparing the document with the copy it began from. The comparison costs what the edit
//! wrote: a shape, a sheet, a section and a row are each behind an `Arc` that an edit replaces only where it writes, so everything whose
//! pointer is the same is skipped, and only the shapes an edit replaced are compared cell by cell.
//!
//! **What it finds.** For one page: the cells of its sheet and of its shapes that were written, added or taken away (by address, so a cell in a
//! row is found by its row's key); the shapes that were added, or whose master changed, which count as new in every cell they hold or
//! inherit ([`Changes::fresh`]); the `ID`s of the shapes that were taken out; and the connectors whose `Connect` rows changed. Two shapes of one
//! list with one `ID` are told apart by their order among the shapes of that chain, and a row with no key by its place among the rows of no key
//! of its section (whose change is a change of the section as a whole): neither is a reason to give up. **When it gives up.** A
//! change outside the page's own shapes and sheet (a style, a master that was not only added, the document's sheet, the colours), a style link of
//! the page's sheet that changed, or a tree too large to index sets [`Changes::whole_page`], and the caller works out every cell of the page,
//! which writes more: the cached value of every cell the page holds that this evaluator gives another result for. Such an answer carries the document
//! the edit started from ([`Changes::since`]), so that the cells an instance inherits are given to it only where the edit changed them, and the masters'
//! cells the edit changed are worked out (`commit.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, Cells, Connect, Document, Formula, FormulaSlot, MAX_SHAPE_DEPTH, Owner, Page, RowKey, Section, SectionKey, Shape,
    ShapeList, ShapeRef, SharedVec, Sheet, SheetRef,
};

use crate::index::ReaderIndex;

/// What an edit changed on one page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// Something that is not a cell of the page's own sheet or shapes changed, or a change could not be attributed: work out the whole page.
    pub whole_page: bool,
    /// The cells written, added or taken away, as sheet and address.
    pub cells: Vec<(SheetRef, CellAddr)>,
    /// The rows, and the sections (`None` for the row), that appeared, went, or took another type (`T`) without a cell of theirs being written: a
    /// row of no cells, or one that is a `RelLineTo` where it was a `LineTo` with the same `X` and `Y`. No cell reports these, and a function that
    /// reads a whole section (the path functions) reads them.
    pub rows: Vec<(SheetRef, SectionKey, Option<RowKey>)>,
    /// Shapes that are new, or have another master than before: every cell they hold, and every cell they inherit, counts as changed.
    pub fresh: Vec<SheetRef>,
    /// The `ID`s of the shapes taken out (nested ones too): a formula that names one now reads nothing.
    pub removed: Vec<u32>,
    /// With [`Changes::whole_page`]: the edit wrote no cell of the page's shapes or sheet and added no shape, and left the document's sheet and the
    /// masters as they were, so the arguments of every elliptical arc (`_ELLIPSE_THETA` and `_ELLIPSE_ECC` read the shape's `Width` and `Height` and
    /// numbers in their own formulas) are what they were. The pass over the whole page then leaves the `C` and `D` of an arc as they are: the pair a
    /// file holds is the application's for those arguments, and the rule differs from it in the classes lists
    /// (changes the curve). Set by `changes_between` for an edit of the styles, themes, colours or fonts alone, and by a caller that
    /// knows its edit is one (`theme.apply`). False when unknown, which works the arcs out like any cell.
    pub arcs_untouched: bool,
    /// With [`Changes::whole_page`]: the document the edit started from, which `changes_between` gives with every whole page and a caller that makes
    /// its own gives when it has it (`theme.apply`). The pass over the whole page works out the cells that instances do not hold and inherit from their
    /// master in it as well: an instance is given such a cell only where the edit changed its result, and the cells of the masters the page's
    /// instances use are worked out where the edit changed theirs. With none, no inherited cell is given to any instance and no master
    /// is worked out.
    pub since: Since,
}

/// The document an edit started from ([`Changes::since`]), shared. Two are equal when both say none, or both hold equal documents: it says what the
/// changes are measured from, and [`Debug`] does not print it.
#[derive(Clone, Default)]
pub struct Since(Option<Arc<Document>>);

impl Since {
    /// The document `doc`, as an edit's start.
    pub fn of(doc: &Document) -> Since {
        Since(Some(Arc::new(doc.clone())))
    }

    /// The document, if one is given.
    pub fn document(&self) -> Option<&Document> {
        self.0.as_deref()
    }
}

impl PartialEq for Since {
    fn eq(&self, other: &Since) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Since {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "Since(the document before)" } else { "Since(none)" })
    }
}

impl Changes {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        !self.whole_page && self.cells.is_empty() && self.rows.is_empty() && self.fresh.is_empty() && self.removed.is_empty()
    }
}

/// Calls `f` with the address and the cell of every cell of a sheet: directly under it, under each section, and in each row.
pub(crate) fn each_cell<'a>(sheet: &'a Sheet, mut f: impl FnMut(CellAddr, &'a Cell)) {
    each_own_cell(sheet, &mut f);
    for section in &sheet.sections {
        each_section_cell(section, &mut f);
    }
}

/// [`each_cell`] for the cells directly under a sheet.
pub(crate) fn each_own_cell<'a>(sheet: &'a Sheet, mut f: impl FnMut(CellAddr, &'a Cell)) {
    crate::work::cells_walk(sheet.cells.len());
    for (name, cell) in &sheet.cells {
        f(CellAddr { section: None, row: None, cell: name.clone() }, cell);
    }
}

/// [`each_cell`] for the cells of one section of a sheet: under the section, then in each row.
pub(crate) fn each_section_cell<'a>(section: &'a Section, mut f: impl FnMut(CellAddr, &'a Cell)) {
    let key = SectionKey { name: section.name.clone(), ix: section.ix };
    crate::work::cells_walk(section.cells.len());
    for (name, cell) in &section.cells {
        f(CellAddr { section: Some(key.clone()), row: None, cell: name.clone() }, cell);
    }
    for row in &section.rows {
        let Some(row_key) = row.key(section.name.as_str()) else { continue };
        crate::work::cells_walk(row.cells.len());
        for (name, cell) in &row.cells {
            f(CellAddr { section: Some(key.clone()), row: Some(row_key.clone()), cell: name.clone() }, cell);
        }
    }
}

/// A cell of a section that a walk of formulas looks at: its address, and its formula for a cell that holds one (`Local`), none for one that inherits it (`Inh`).
type FormulaCell = (CellAddr, Option<Arc<Formula>>);

/// The most cells [`SectionFormulas`] keeps for one commit, over all the sections it has listed: past it a section is walked each time it is asked for, as before.
const MAX_LISTED_FORMULAS: usize = 1 << 18;

/// The cells of the sections of a page that hold a formula or inherit one, **listed once for a section however many sheets hold it**: the copies of a
/// shape share their sections (a paste, a duplicate, a file's thousand copies of one connector), and the walks of a commit (the page's index, the cells of a sheet that
/// read its own cells) walked every cell of a shared section of thousands of rows of constants for each copy: ten thousand connectors sharing a route of 4,000 rows were
/// hundreds of millions of cells for a move that changed only their ends. A section is known by its pointer, and the list holds the section, so that the pointer is
/// never another's while the list is kept; a list is made by a walk of the section's cells, and is what that walk would hand out, in its order.
#[derive(Default)]
pub(crate) struct SectionFormulas {
    by: HashMap<usize, (Arc<Section>, std::rc::Rc<[FormulaCell]>)>,
    kept: usize,
}

impl SectionFormulas {
    /// The cells of `section` that hold a formula or inherit one, in the order [`each_section_cell`] meets them.
    fn of(&mut self, section: &Arc<Section>) -> std::rc::Rc<[FormulaCell]> {
        let key = Arc::as_ptr(section) as usize;
        if let Some((_, list)) = self.by.get(&key) {
            return std::rc::Rc::clone(list);
        }
        let mut list: Vec<FormulaCell> = Vec::new();
        each_section_cell(section, |addr, cell| match &cell.formula {
            FormulaSlot::Local(formula) => list.push((addr, Some(Arc::clone(formula)))),
            FormulaSlot::Inherit => list.push((addr, None)),
            _ => {}
        });
        let list: std::rc::Rc<[FormulaCell]> = list.into();
        if self.kept.saturating_add(list.len()) <= MAX_LISTED_FORMULAS {
            self.kept = self.kept.saturating_add(list.len());
            self.by.insert(key, (Arc::clone(section), std::rc::Rc::clone(&list)));
        }
        list
    }
}

/// Calls `f` with the address of every cell of `sheet` that holds a formula (and it) or inherits one (and none), in the order [`each_cell`] meets them: the sheet's own
/// cells walked, and each section's cells from `memo`, listed once for the section.
pub(crate) fn each_formula(sheet: &Sheet, memo: &mut SectionFormulas, mut f: impl FnMut(&CellAddr, Option<&Arc<Formula>>)) {
    each_own_cell(sheet, |addr, cell| match &cell.formula {
        FormulaSlot::Local(formula) => f(&addr, Some(formula)),
        FormulaSlot::Inherit => f(&addr, None),
        _ => {}
    });
    for section in &sheet.sections {
        for (addr, formula) in memo.of(section).iter() {
            f(addr, formula.as_ref());
        }
    }
}

/// The shapes of one page, each with its place, the `ID`s from the top down to it (its identity across an edit that removes a sibling, which
/// moves the places of the ones after it), and the shape.
struct Placed<'a> {
    path: Vec<u32>,
    shape: &'a Arc<Shape>,
}

/// The most numbers the places and `ID`s of all the shapes of a part may come to (a shape deep in a tree holds a copy of the way to it): past
/// it a part is not compared by `ID`, and the page is worked out whole. A page of ten thousand shapes in a tree of depth 20 is two million.
const MAX_INDEXED_NUMBERS: usize = 8_000_000;

/// A shape's identity across an edit: the `ID`s from the top down to it, and how many shapes before it (a group before its members, a shape
/// before its next sibling) have that same chain. The count is 0 for every shape of a tree whose lists hold each `ID` once.
type ShapeKey = (Vec<u32>, u32);

/// Every shape of a part, a group before its members, found with a stack, none deeper than [`MAX_SHAPE_DEPTH`]. `None` when the tree is too
/// large to index.
///
/// **Two shapes of one list with one `ID`** (a file may hold a deleted member, `Del="1"`, beside a live one of its `ID`, and the reader
/// keeps both) are told apart by their order: the first is `(chain, 0)`, the next `(chain, 1)`, and so on, so an edit that leaves the tree's shape as it
/// was (a move, a cell written) is compared shape by shape. Such a page used to be worked out whole, which wrote the cached value of every cell
/// this evaluator gives another result for and added the inherited cells of every instance on the page: rows of shapes the edit never reached.
/// Only an edit that takes out or adds a shape of a repeated `ID` can pair two shapes that are not the same; their cells then count as changed,
/// which works out more than it needs and never less.
fn index_shapes(top: &ShapeList) -> Option<HashMap<ShapeKey, Placed<'_>>> {
    let mut out: HashMap<ShapeKey, Placed<'_>> = HashMap::new();
    // The next count of each chain that was met more than once: only those are kept, so a tree of distinct `ID`s holds no second copy of its chains.
    let mut repeated: HashMap<Vec<u32>, u32> = HashMap::new();
    let mut numbers = 0_usize;
    // (the list, the next place in it, the places so far, the IDs so far)
    let mut stack: Vec<(&ShapeList, usize, Vec<u32>, Vec<u32>)> = vec![(top, 0, Vec::new(), Vec::new())];
    while let Some((list, next, places, ids)) = stack.last_mut() {
        let Some(shape) = list.get(*next) else {
            stack.pop();
            continue;
        };
        let place = u32::try_from(*next).ok()?;
        *next += 1;
        let mut path = places.clone();
        path.push(place);
        let mut chain = ids.clone();
        chain.push(shape.id);
        numbers = numbers.checked_add(path.len().checked_mul(2)?)?;
        if numbers > MAX_INDEXED_NUMBERS {
            return None;
        }
        let placed = Placed { path: path.clone(), shape };
        let first = (chain.clone(), 0);
        if out.contains_key(&first) {
            // A chain met before: the copy of it the count is kept under is a number of its own.
            numbers = numbers.checked_add(chain.len())?;
            if numbers > MAX_INDEXED_NUMBERS {
                return None;
            }
            let count = repeated.entry(chain.clone()).or_insert(1);
            out.insert((chain.clone(), *count), placed);
            *count = count.checked_add(1)?;
        } else {
            out.insert(first, placed);
        }
        if path.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0, path, chain));
        }
    }
    Some(out)
}

/// Pushes every cell of a sheet as changed, and its sections and rows.
fn push_all(out: &mut Changes, sheet_ref: &SheetRef, sheet: &Sheet) {
    each_cell(sheet, |addr, _| out.cells.push((sheet_ref.clone(), addr)));
    for section in &sheet.sections {
        push_places(out, sheet_ref, section);
    }
}

/// The cells of `a` and `b` (the same container before and after) that differ.
fn diff_cells(out: &mut Vec<(SheetRef, CellAddr)>, sheet: &SheetRef, a: &Cells, b: &Cells, place: &CellAddr) {
    let at = |name: &CellName| CellAddr { section: place.section.clone(), row: place.row.clone(), cell: name.clone() };
    for (name, after) in b {
        if a.get(name) != Some(after) {
            out.push((sheet.clone(), at(name)));
        }
    }
    for name in a.keys() {
        if !b.contains_key(name) {
            out.push((sheet.clone(), at(name)));
        }
    }
}

/// The cells of two copies of one sheet that differ. `false` when the sheets differ in a way that is not a cell (a style link): every cell
/// of the sheet may then have changed, and the caller treats the sheet as new.
fn diff_sheets(out: &mut Changes, sheet_ref: &SheetRef, a: &Sheet, b: &Sheet) -> bool {
    if a.links != b.links {
        return false;
    }
    let nowhere = CellAddr { section: None, row: None, cell: CellName::new("") };
    diff_cells(&mut out.cells, sheet_ref, &a.cells, &b.cells, &nowhere);
    for (at, after) in b.sections.iter().enumerate() {
        let before = match a.sections.get(at) {
            Some(same_place) if same_place.name == after.name && same_place.ix == after.ix => Some(same_place),
            _ => a.sections.iter().find(|s| s.name == after.name && s.ix == after.ix),
        };
        let key = SectionKey { name: after.name.clone(), ix: after.ix };
        let Some(before) = before else {
            push_section(out, sheet_ref, after);
            continue;
        };
        if Arc::ptr_eq(before, after) {
            continue;
        }
        if before.del != after.del {
            push_section(out, sheet_ref, before);
            push_section(out, sheet_ref, after);
            continue;
        }
        let in_section = CellAddr { section: Some(key.clone()), row: None, cell: CellName::new("") };
        diff_cells(&mut out.cells, sheet_ref, &before.cells, &after.cells, &in_section);
        // A row with no key (a row of a section keyed by name that has no `N`, which a file may hold) is named by no reference: it is paired with the
        // row of no key at its place among them, and one that differs, comes or goes changes the section as a whole (what a function that reads every
        // row reads), as a row that took another type does. Such a sheet used to be no sheet `changes_between` could compare.
        if !same_keyless_rows(before, after) {
            out.rows.push((sheet_ref.clone(), key.clone(), None));
        }
        for row in &after.rows {
            let Some(row_key) = row.key(after.name.as_str()) else { continue };
            let in_row = CellAddr { section: Some(key.clone()), row: Some(row_key.clone()), cell: CellName::new("") };
            let place = (sheet_ref.clone(), key.clone(), Some(row_key.clone()));
            match before.row(&row_key) {
                Some(old) if Arc::ptr_eq(old, row) => {}
                Some(old) if old.del == row.del => {
                    diff_cells(&mut out.cells, sheet_ref, &old.cells, &row.cells, &in_row);
                    // A row that took another type changed what it draws and no cell says so.
                    if old.row_type != row.row_type {
                        out.rows.push(place);
                    }
                }
                Some(old) => {
                    diff_cells(&mut out.cells, sheet_ref, &old.cells, &Default::default(), &in_row);
                    diff_cells(&mut out.cells, sheet_ref, &Default::default(), &row.cells, &in_row);
                    out.rows.push(place);
                }
                None => {
                    diff_cells(&mut out.cells, sheet_ref, &Default::default(), &row.cells, &in_row);
                    // A row with no cells is a row all the same.
                    out.rows.push(place);
                }
            }
        }
        for row in &before.rows {
            let Some(row_key) = row.key(before.name.as_str()) else { continue };
            if after.row(&row_key).is_none() {
                let in_row = CellAddr { section: Some(key.clone()), row: Some(row_key.clone()), cell: CellName::new("") };
                diff_cells(&mut out.cells, sheet_ref, &row.cells, &Default::default(), &in_row);
                out.rows.push((sheet_ref.clone(), key.clone(), Some(row_key)));
            }
        }
    }
    for before in &a.sections {
        if !b.sections.iter().any(|s| s.name == before.name && s.ix == before.ix) {
            push_section(out, sheet_ref, before);
        }
    }
    true
}

/// Whether the rows of no key of two copies of one section are the same rows, place by place among them.
fn same_keyless_rows(before: &Section, after: &Section) -> bool {
    let keyless = |s: &Section| s.rows.iter().filter(|r| r.key(s.name.as_str()).is_none()).cloned().collect::<Vec<Arc<diacraft_doc::Row>>>();
    let (a, b) = (keyless(before), keyless(after));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| Arc::ptr_eq(x, y) || x == y)
}

/// Pushes every cell of a section and its rows as changed, and the section and its rows themselves.
fn push_section(out: &mut Changes, sheet_ref: &SheetRef, section: &Section) {
    let key = SectionKey { name: section.name.clone(), ix: section.ix };
    for name in section.cells.keys() {
        out.cells.push((sheet_ref.clone(), CellAddr { section: Some(key.clone()), row: None, cell: name.clone() }));
    }
    for row in &section.rows {
        let Some(row_key) = row.key(section.name.as_str()) else { continue };
        for name in row.cells.keys() {
            out.cells.push((sheet_ref.clone(), CellAddr { section: Some(key.clone()), row: Some(row_key.clone()), cell: name.clone() }));
        }
    }
    push_places(out, sheet_ref, section);
}

/// Pushes a section, and each of its rows, as appeared, gone or retyped (see [`Changes::rows`]).
fn push_places(out: &mut Changes, sheet_ref: &SheetRef, section: &Section) {
    let key = SectionKey { name: section.name.clone(), ix: section.ix };
    out.rows.push((sheet_ref.clone(), key.clone(), None));
    for row in &section.rows {
        if let Some(row_key) = row.key(section.name.as_str()) {
            out.rows.push((sheet_ref.clone(), key.clone(), Some(row_key)));
        }
    }
}

/// Whether `after` is `before` and nothing taken away or changed: the same elements in the same places, and perhaps more at the end.
fn only_appended<T: PartialEq>(before: &SharedVec<Arc<T>>, after: &SharedVec<Arc<T>>) -> bool {
    after.len() >= before.len() && before.iter().zip(after.iter()).all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
}

/// The two ends a `Connect` row's `FromCell` names: `BeginX` and `BeginY` are one end, and `EndX` and `EndY`.
pub(crate) fn end_cells(from_cell: &str) -> Vec<&str> {
    match from_cell {
        "BeginX" | "BeginY" => vec!["BeginX", "BeginY"],
        "EndX" | "EndY" => vec!["EndX", "EndY"],
        other => vec![other],
    }
}

/// What changed on page `page` between `before` and `after`. The documents are the same drawing, one edit apart.
///
/// The comparison costs what the edit wrote **when the two share what the edit did not write and no two shapes of a list hold one `ID`**, which this finds by a pass over the
/// page's shapes for each call (the `ID`s are indexed: a chain of them for each shape). A caller that keeps a [`ReaderIndex`] from one edit to the next uses
/// [`changes_between_with`], which remembers the page it found free of such `ID`s and compares the two copies in step, skipping every list and shape that is the same allocation.
pub fn changes_between(before: &Document, after: &Document, page: u32) -> Changes {
    changes_between_in(None, before, after, page)
}

/// [`changes_between`] with a [`ReaderIndex`], which remembers the page whose shapes it indexed by `ID` and found no two alike, so that the next comparison of that page
/// with an edit of it need not do the pass again: two copies of the page that hold the same `ID`s in the same places are compared in step, and what is the same allocation is
/// skipped, so the comparison costs what the edit replaced (it was a pass over 12,000 shapes, with a vector and a hash entry for each, for every edit). The answer
/// is the answer of [`changes_between`], the same cells in the same order (`affected/tests.rs` holds the two to it over generated pages, with duplicate `ID`s among them).
pub fn changes_between_with(index: &mut ReaderIndex, before: &Document, after: &Document, page: u32) -> Changes {
    changes_between_in(Some(index.known_shapes()), before, after, page)
}

fn changes_between_in(known: Option<&mut KnownShapes>, before: &Document, after: &Document, page: u32) -> Changes {
    let mut changes = compared(known, before, after, page);
    // A page worked out whole is measured from the document before: what the pass gives an instance it inherits is what the edit changed.
    if changes.whole_page {
        changes.since = Since::of(before);
    }
    changes
}

fn compared(known: Option<&mut KnownShapes>, before: &Document, after: &Document, page: u32) -> Changes {
    let mut changes = Changes::default();
    let find = |doc: &Document| doc.pages.iter().find(|p| p.id == page).cloned();
    let (Some(old), Some(new)) = (find(before), find(after)) else {
        changes.whole_page = true;
        return changes;
    };
    // Anything outside the page's own sheet and shapes: a change is not attributed to cells, and the page is worked out whole.
    let rest_same = before.styles == after.styles
        && before.themes == after.themes
        && before.colors == after.colors
        && before.fonts == after.fonts
        && before.sheet == after.sheet
        && (before.masters == after.masters || only_appended(&before.masters, &after.masters));
    if !rest_same {
        changes.whole_page = true;
        // A style, a theme, a colour or a font changed, and the document's sheet and the masters did not (or only gained masters): the sizes of the
        // page's shapes come from the shapes and their masters, never from a style (`Width` and `Height` are cells of a shape alone), so unless the page was
        // edited as well the arguments of every elliptical arc are as they were, and a pass over the whole page leaves their pairs be.
        let looks_only = before.sheet == after.sheet && (before.masters == after.masters || only_appended(&before.masters, &after.masters));
        if looks_only && (Arc::ptr_eq(&old, &new) || page_unchanged(&old, &new, page)) {
            changes.arcs_untouched = true;
        }
        return changes;
    }
    if Arc::ptr_eq(&old, &new) {
        return changes;
    }
    compare_page(&old, &new, page, &mut changes, known);
    changes
}

/// Whether the page's own sheet and shapes are as they were, cell by cell, rows and shapes included: the comparison finds nothing and has no need
/// to give up. (A page that is not the same `Arc` can still be the same page; a comparison that cannot attribute its changes says no.)
fn page_unchanged(old: &Arc<Page>, new: &Arc<Page>, page: u32) -> bool {
    let mut found = Changes::default();
    compare_page(old, new, page, &mut found, None);
    found == Changes::default()
}

/// What `changes_between_with` remembers from one call to the next: a page whose shapes were indexed by `ID` (`index_shapes`) within the size the index allows (two of one list
/// alike are told apart by their order), so that a copy of it that holds the same `ID`s in the same places is one too, without the pass.
#[derive(Debug, Default)]
pub(crate) struct KnownShapes {
    indexed: Option<Arc<Page>>,
}

impl KnownShapes {
    /// Whether `page`'s shapes are as `index_shapes` wants them (an index is made of them: the tree is within its size): by the page it remembers where the two hold the
    /// same `ID`s in the same places, and otherwise by the pass. Remembers `page` when it is.
    fn covers(&mut self, page: &Arc<Page>) -> bool {
        if let Some(known) = &self.indexed
            && (Arc::ptr_eq(known, page) || same_ids(&known.shapes, &page.shapes, 0))
        {
            self.indexed = Some(Arc::clone(page));
            return true;
        }
        self.indexed = index_shapes(&page.shapes).map(|_| Arc::clone(page));
        self.indexed.is_some()
    }
}

/// Whether two lists of shapes hold the same `ID`s in the same places, as far as `index_shapes` looks (down to [`MAX_SHAPE_DEPTH`]). Lists, chunks and shapes that are the same
/// allocation are the same and are skipped.
fn same_ids(old: &ShapeList, new: &ShapeList, depth: usize) -> bool {
    if old.is_same_list_as(new) {
        return true;
    }
    if old.len() != new.len() {
        return false;
    }
    for (a_chunk, b_chunk) in old.chunk_arcs().iter().zip(new.chunk_arcs()) {
        if a_chunk.len() != b_chunk.len() {
            return false;
        }
        if Arc::ptr_eq(a_chunk, b_chunk) {
            continue;
        }
        for (a, b) in a_chunk.iter().zip(b_chunk.iter()) {
            if Arc::ptr_eq(a, b) {
                continue;
            }
            if a.id != b.id || (depth + 1 < MAX_SHAPE_DEPTH && !same_ids(&a.children, &b.children, depth + 1)) {
                return false;
            }
        }
    }
    true
}

/// What one shape that is not the same allocation as it was changed: it is new in every cell if its master or its `Del` changed, and else its cells that differ. `false`
/// when the comparison of its sheets gave up (they differ in a way that is not a cell): the shape counts as new in every cell, as the comparison by `ID` says.
fn shape_changes(was: &Shape, now: &Shape, sheet_ref: &SheetRef, changes: &mut Changes) -> bool {
    if was.master != now.master || was.master_shape != now.master_shape || was.del != now.del {
        changes.fresh.push(sheet_ref.clone());
        push_all(changes, sheet_ref, &now.sheet);
        return true;
    }
    if !Arc::ptr_eq(&was.sheet, &now.sheet) && !diff_sheets(changes, sheet_ref, &was.sheet, &now.sheet) {
        changes.fresh.push(sheet_ref.clone());
        push_all(changes, sheet_ref, &now.sheet);
        return false;
    }
    true
}

/// The changes between two lists of shapes that hold the same `ID`s in the same places, found in step: a list, a chunk or a shape that is the same allocation is skipped, and the
/// shapes that are not are compared as [`compare_page`] compares them, in the same order (a group before its members). `None` when the lists are not the same shapes in the same
/// places. `Some(true)` when no comparison of two sheets gave up, `Some(false)` when one did (a shape of the two copies that holds equal content may then have been counted as new:
/// see [`compare_page`]).
fn compare_in_step(old: &ShapeList, new: &ShapeList, owner: Owner, path: &mut Vec<u32>, out: &mut Changes) -> Option<bool> {
    if old.is_same_list_as(new) {
        return Some(true);
    }
    if old.len() != new.len() {
        return None;
    }
    let mut calm = true;
    let mut base = 0_usize;
    for (a_chunk, b_chunk) in old.chunk_arcs().iter().zip(new.chunk_arcs()) {
        if a_chunk.len() != b_chunk.len() {
            return None;
        }
        if !Arc::ptr_eq(a_chunk, b_chunk) {
            for (k, (a, b)) in a_chunk.iter().zip(b_chunk.iter()).enumerate() {
                if Arc::ptr_eq(a, b) {
                    continue;
                }
                if a.id != b.id {
                    return None;
                }
                path.push(u32::try_from(base.saturating_add(k)).ok()?);
                let sheet_ref = SheetRef::Shape(ShapeRef { owner, path: path.clone() });
                calm &= shape_changes(a, b, &sheet_ref, out);
                let within = if path.len() < MAX_SHAPE_DEPTH { compare_in_step(&a.children, &b.children, owner, path, out) } else { Some(true) };
                path.pop();
                calm &= within?;
            }
        }
        base = base.saturating_add(a_chunk.len());
    }
    Some(calm)
}

fn compare_page(old: &Arc<Page>, new: &Arc<Page>, page: u32, changes: &mut Changes, known: Option<&mut KnownShapes>) {
    let owner = Owner::Page(page);
    if !Arc::ptr_eq(&old.sheet, &new.sheet) && !diff_sheets(changes, &SheetRef::Page(page), &old.sheet, &new.sheet) {
        changes.whole_page = true;
        return;
    }
    // The shapes, in step, when the copy before is one whose `ID`s are known to be in order and the two hold the same `ID`s in the same places: the answer is the one the
    // comparison by `ID` below gives, and costs what the edit replaced.
    let mut in_step = false;
    #[cfg(test)]
    let known_was_asked = known.is_some();
    if let Some(known) = known
        && known.covers(old)
    {
        let mut found = Changes::default();
        if let Some(calm) = compare_in_step(&old.shapes, &new.shapes, owner, &mut Vec::new(), &mut found) {
            #[cfg(test)]
            {
                crate::index::tally::bump(&crate::index::tally::IN_STEP);
                if !calm {
                    crate::index::tally::bump(&crate::index::tally::GAVE_UP_IN_STEP);
                }
            }
            // A comparison of two sheets gave up, and the two lists hold what is the same in content: the comparison by `ID` skips lists that are equal and says nothing of them.
            if !(calm || old.shapes != new.shapes) {
                found = Changes::default();
            }
            changes.cells.extend(found.cells);
            changes.rows.extend(found.rows);
            changes.fresh.extend(found.fresh);
            known.indexed = Some(Arc::clone(new));
            in_step = true;
        }
    }
    if !in_step && old.shapes != new.shapes {
        #[cfg(test)]
        if known_was_asked {
            crate::index::tally::bump(&crate::index::tally::BY_ID);
        }
        let (Some(before), Some(after)) = (index_shapes(&old.shapes), index_shapes(&new.shapes)) else {
            changes.whole_page = true;
            return;
        };
        // The members of a shape taken out are taken out with it, each under its own chain, so each is found here.
        changes.removed.extend(before.keys().filter(|key| !after.contains_key(*key)).filter_map(|(chain, _)| chain.last().copied()));
        // In the order of the places, so that what is worked out and written does not depend on the hash map.
        let mut now_shapes: Vec<(&ShapeKey, &Placed<'_>)> = after.iter().collect();
        now_shapes.sort_by(|a, b| a.1.path.cmp(&b.1.path));
        for (key, now) in now_shapes {
            let sheet_ref = SheetRef::Shape(ShapeRef { owner, path: now.path.clone() });
            let Some(was) = before.get(key) else {
                changes.fresh.push(sheet_ref.clone());
                push_all(changes, &sheet_ref, &now.shape.sheet);
                continue;
            };
            if Arc::ptr_eq(was.shape, now.shape) {
                continue;
            }
            if was.shape.master != now.shape.master || was.shape.master_shape != now.shape.master_shape || was.shape.del != now.shape.del {
                changes.fresh.push(sheet_ref.clone());
                push_all(changes, &sheet_ref, &now.shape.sheet);
                continue;
            }
            if !Arc::ptr_eq(&was.shape.sheet, &now.shape.sheet) && !diff_sheets(changes, &sheet_ref, &was.shape.sheet, &now.shape.sheet) {
                changes.fresh.push(sheet_ref.clone());
                push_all(changes, &sheet_ref, &now.shape.sheet);
            }
        }
        changes.removed.sort_unstable();
        changes.removed.dedup();
    }
    if old.connects != new.connects {
        connect_changes(old, new, page, changes);
    }
}

/// The end cells of the connectors whose `Connect` rows were added or taken away.
fn connect_changes(old: &Page, new: &Page, page: u32, changes: &mut Changes) {
    type Link = (u32, Option<Arc<str>>, Option<u32>, u32, Option<Arc<str>>, Option<u32>);
    let row = |c: &Connect| -> Link { (c.from_sheet, c.from_cell.clone(), c.from_part, c.to_sheet, c.to_cell.clone(), c.to_part) };
    let before: HashSet<Link> = old.connects.iter().map(row).collect();
    let after: HashSet<Link> = new.connects.iter().map(row).collect();
    let mut shapes: HashMap<u32, Option<ShapeRef>> = HashMap::new();
    for (from, from_cell, ..) in before.symmetric_difference(&after) {
        let Some(from_cell) = from_cell else { continue };
        let at = shapes.entry(*from).or_insert_with(|| find_in(&new.shapes, Owner::Page(page), *from));
        let Some(at) = at else { continue };
        for name in end_cells(from_cell) {
            changes.cells.push((SheetRef::Shape(at.clone()), CellAddr::direct(name)));
        }
    }
}

/// The first shape of a part with this `ID`, found with a stack.
fn find_in(top: &ShapeList, owner: Owner, id: u32) -> Option<ShapeRef> {
    let mut stack: Vec<(&ShapeList, usize, Vec<u32>)> = vec![(top, 0, Vec::new())];
    while let Some((list, next, places)) = stack.last_mut() {
        let Some(shape) = list.get(*next) else {
            stack.pop();
            continue;
        };
        let place = u32::try_from(*next).ok()?;
        *next += 1;
        let mut path = places.clone();
        path.push(place);
        if shape.id == id {
            return Some(ShapeRef { owner, path });
        }
        if path.len() < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
            stack.push((&shape.children, 0, path));
        }
    }
    None
}
