//! The readers of the cells of a page, kept from one edit to the next.
//!
//! **Why.** To say which cells an edit reaches, [`crate::affected`] needs, for a cell, the cells that read it through another sheet (a qualified
//! reference, a function that carries a point between frames, a `Connect` row), the cells that read what their text does not name, and the cells whose
//! reference finds no sheet. Those are found by asking every formula of the page whether it reads beyond its own sheet, which an edit of one shape of
//! twelve thousand (168,000 formulas, 1.6 million small objects of the model) did again for every edit, and which was nine tenths of the work of the
//! edit (measured it with a profiler). The answer for a formula does not change when another shape is edited, so the **graph is kept**: it is
//! made once for a page, and an edit finds the shapes it replaced, by pointer, and puts right what it read of those and nothing else.
//!
//! **What is kept.** A [`ReaderIndex`] holds the graph of the page the last edit was on, the page it was made for (the allocation, held, so that a pointer to
//! it is never another page's), and what the graph reads of the rest of the document: the masters, the styles and the names of the pages. For each sheet that
//! reads beyond itself it holds what the sheet put in the graph, so that the sheet's old entries can be taken out when its formulas change. A page with no
//! formula that reads beyond its sheet holds an empty graph, and a document that reads none is held for nothing. **Beside it** (F40) it holds
//! the graphs of the pages a change reached through a cell that names a page (`Pages[..]`), each made when a change first reached a cell of the page and kept in step
//! the same way, up to [`MAX_KEPT_PAGES`]; and the listing of the sheets of every page that name a page, which `pages.rs` makes and keeps.
//!
//! **How it is brought up to date** ([`ReaderIndex::sync`]), whatever the edits since it was made (a command's edit, its undo, a redo, a file opened over
//! the one it was made for):
//!
//! * the page, the masters, the styles and the names of the pages are looked at by pointer first (an edit copies only what it writes), and the shapes
//!   of the page are walked beside the shapes the graph was made for, **skipping every list and every shape that is the same allocation**. The walk costs
//!   what the edit replaced, a chunk of 64 shapes and the shape;
//! * a shape that is the same shape in the same place (its `ID`, names, kind and master links are as they were) whose sheet is another allocation is
//!   **a replaced sheet**; anything else that differs (a shape added, taken out, moved, given another master or another name, a master, a style or a
//!   page name changed, the `Connect` rows) is **a change the graph does not follow**, and it is made again whole, as every edit made it before;
//! * a replaced sheet whose formulas are as they were (an edit that writes values: a move, a fill, the results of a recalculation) changes nothing
//!   in the graph. One whose formulas changed is classified again, and its old entries are taken out first;
//! * a replaced sheet that gained, lost or retyped a row or a section changes what a reference to a row of it may be spelled as, so the sheets that read
//!   it through another are classified again too.
//!
//! **The same graph a walk would have made.** Entries are kept in the order a walk of the whole page lists them (the sheets in the order of
//! `walk_page`, the cells of a sheet in the order of its formulas, the inherited formulas after them, the `Connect` rows after all), so that the cells an
//! edit reaches are handed out in the order they always were. `affected/oracle.rs` keeps the walk this replaced, and the tests hold the two to the same
//! cells, in the same order, over generated pages and edits.
//!
//! **What it costs, and what it is charged.** The graph is charged as the walk charged it, each entry by the same figure, to the budget of the commit
//! that makes it; a commit that uses a graph that is kept is charged the whole graph first ([`ReaderIndex::held_bytes`]), so a graph that was made
//! within a budget is within it again. What the engine's session holds of it is counted with the session's (`Session::held_memory`). A graph that is more
//! than [`MAX_KEPT_BYTES`] is not kept, and the next edit makes it again. A budget that runs out while a graph is made or brought up to date leaves none.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellName, Document, FormulaSlot, MAX_SHAPE_DEPTH, Master, Owner, Page, Shape, ShapeList, ShapeRef, SharedVec, Sheet, SheetRef, Style,
};

use crate::affected::{Found, Key, MasterCross, Reader, classify_sheet, pose_keys};
use crate::changes::{KnownShapes, SectionFormulas, each_own_cell, each_section_cell, end_cells};
use crate::eval::Evaluator;
use crate::pages::PageLinks;
use crate::recalc::walk_page;
use crate::rules::KEY_BYTES;

/// What the tests count of the paths a graph is brought up to date by, so that a property test can say it went down every one of them.
#[cfg(test)]
pub(crate) mod tally {
    use std::cell::Cell;
    use std::thread::LocalKey;

    thread_local! {
        /// Graphs made by a walk of the whole page.
        pub static MADE: Cell<u64> = const { Cell::new(0) };
        /// Graphs kept as they were: the page is the very allocation the graph was made for.
        pub static SAME_PAGE: Cell<u64> = const { Cell::new(0) };
        /// Graphs brought up to date by what the edits since replaced.
        pub static PUT_RIGHT: Cell<u64> = const { Cell::new(0) };
        /// Graphs given up for a change they do not follow, and made again.
        pub static GIVEN_UP: Cell<u64> = const { Cell::new(0) };
        /// Sheets whose formulas changed, classified again.
        pub static RECLASSIFIED: Cell<u64> = const { Cell::new(0) };
        /// Sheets replaced whose formulas were the same.
        pub static VALUES_ONLY: Cell<u64> = const { Cell::new(0) };
        /// Sheets that gained, lost or retyped a row, which put right the sheets that read them.
        pub static ROWS_CHANGED: Cell<u64> = const { Cell::new(0) };
        /// Comparisons of a page's shapes made in step with the copy before.
        pub static IN_STEP: Cell<u64> = const { Cell::new(0) };
        /// Comparisons of a page's shapes made by `ID` (the pass over both copies), by `changes_between_with`.
        pub static BY_ID: Cell<u64> = const { Cell::new(0) };
        /// Comparisons in step in which a comparison of two sheets gave up.
        pub static GAVE_UP_IN_STEP: Cell<u64> = const { Cell::new(0) };
    }

    pub fn bump(counter: &'static LocalKey<Cell<u64>>) {
        counter.with(|c| c.set(c.get() + 1));
    }

    pub fn get(counter: &'static LocalKey<Cell<u64>>) -> u64 {
        counter.with(Cell::get)
    }
}

/// The most a graph that is kept may be charged, as accounted: past it the graph is made again by each edit, as it was before it was kept. A figure
/// to bound what a session holds of it, about 125,000 entries (each is held for far less than the 512 bytes it is charged).
pub const MAX_KEPT_BYTES: u64 = 64 << 20;

/// What a page's graph is ordered by: the phase in the high half and the place in the low. The formulas of the sheets are phase 0, in the order
/// of the walk of the page; the `Connect` rows are phase 1, after every sheet.
const PHASE_SHIFT: u32 = 32;

/// The place of the `ordinal`-th formula of a sheet in a walk of the page.
pub(crate) fn walk_ord(ordinal: u32) -> u64 {
    u64::from(ordinal)
}

/// The place of the `seq`-th end of a `Connect` row.
pub(crate) fn connect_ord(seq: u32) -> u64 {
    (1 << PHASE_SHIFT) | u64::from(seq)
}

/// One reader in the graph, and where a walk of the page found it.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    ord: u64,
    pub(crate) reader: Reader,
    /// For a reader whose reference finds no sheet: the `ID` a plain `Sheet.<id>` names (`affected::Found::dangling`), else `None`.
    pub(crate) named: Option<u32>,
}

impl Entry {
    pub(crate) fn new(ord: u64, reader: Reader) -> Entry {
        Entry { ord, reader, named: None }
    }

    pub(crate) fn phase(&self) -> u64 {
        self.ord >> PHASE_SHIFT
    }
}

/// Where a sheet is in a walk of the page: the page's own sheet first, then the shapes group before member (their paths, in order).
fn rank(sheet: &SheetRef) -> (u8, &[u32]) {
    match sheet {
        SheetRef::Page(_) => (0, &[]),
        SheetRef::Shape(at) => (1, at.path.as_slice()),
        _ => (2, &[]),
    }
}

/// What the entries of a list are grouped by: the phase and, in phase 0, the sheet. The entries of a sheet are together in a list.
fn class(e: &Entry) -> (u64, (u8, &[u32])) {
    let phase = e.phase();
    (phase, if phase == 0 { rank(&e.reader.0) } else { (0, &[]) })
}

fn compare(a: &Entry, b: &Entry) -> Ordering {
    class(a).cmp(&class(b)).then(a.ord.cmp(&b.ord))
}

/// Puts `entry` where a walk would have listed it, after the entries that are as early.
fn insert_sorted(list: &mut Vec<Entry>, entry: Entry) {
    let at = list.partition_point(|e| compare(e, &entry) != Ordering::Greater);
    list.insert(at, entry);
}

/// Takes out the entries of the sheet `sheet` (phase 0), which are together.
fn remove_sheet(list: &mut Vec<Entry>, sheet: &SheetRef) {
    let probe = (0_u64, rank(sheet));
    let from = list.partition_point(|e| class(e) < probe);
    let to = list.partition_point(|e| class(e) <= probe);
    list.drain(from..to);
}

/// What one sheet put in the graph: where, so that it can be taken out.
#[derive(Debug)]
struct Contribution {
    keys: Vec<Key>,
    dynamic: bool,
    dangling: bool,
    bytes: u64,
}

/// The readers of one page: see the module documentation.
#[derive(Debug, Default)]
pub(crate) struct Graph {
    /// The readers of a cell through a qualified reference, a frame function or a `Connect` row, by the cell they read.
    pub(crate) cross: HashMap<Key, Vec<Entry>>,
    /// The readers of cells their text does not name.
    pub(crate) dynamic: Vec<Entry>,
    /// The readers with a qualified reference that finds no sheet.
    pub(crate) dangling: Vec<Entry>,
    contributions: HashMap<SheetRef, Contribution>,
    /// What the graph is charged, as accounted.
    pub(crate) bytes: u64,
}

impl Graph {
    /// Puts what a sheet found in the graph.
    fn install(&mut self, sheet: &SheetRef, found: Found) {
        let Found { cross, dynamic, dangling, bytes } = found;
        if cross.is_empty() && dynamic.is_empty() && dangling.is_empty() {
            self.bytes = self.bytes.saturating_add(bytes);
            return;
        }
        let mut contribution = Contribution { keys: Vec::new(), dynamic: !dynamic.is_empty(), dangling: !dangling.is_empty(), bytes };
        let mut placed: HashSet<Key> = HashSet::new();
        for (key, reader, ord) in cross {
            if placed.insert(key.clone()) {
                contribution.keys.push(key.clone());
            }
            insert_sorted(self.cross.entry(key).or_default(), Entry::new(ord, reader));
        }
        for (reader, ord) in dynamic {
            insert_sorted(&mut self.dynamic, Entry::new(ord, reader));
        }
        for (reader, ord, named) in dangling {
            insert_sorted(&mut self.dangling, Entry { named, ..Entry::new(ord, reader) });
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.contributions.insert(sheet.clone(), contribution);
    }

    /// Puts the `Connect` rows' entries in the graph, after the sheets' (they are not any sheet's to take out: a change to them makes the graph again).
    fn install_connects(&mut self, found: Found) {
        for (key, reader, ord) in found.cross {
            insert_sorted(self.cross.entry(key).or_default(), Entry::new(ord, reader));
        }
        self.bytes = self.bytes.saturating_add(found.bytes);
    }

    /// Takes out what the sheet put in the graph. → what it was charged.
    fn remove(&mut self, sheet: &SheetRef) -> u64 {
        let Some(contribution) = self.contributions.remove(sheet) else { return 0 };
        for key in &contribution.keys {
            let Some(list) = self.cross.get_mut(key) else { continue };
            remove_sheet(list, sheet);
            if list.is_empty() {
                self.cross.remove(key);
            }
        }
        if contribution.dynamic {
            remove_sheet(&mut self.dynamic, sheet);
        }
        if contribution.dangling {
            remove_sheet(&mut self.dangling, sheet);
        }
        self.bytes = self.bytes.saturating_sub(contribution.bytes);
        contribution.bytes
    }

    /// The sheets that read a cell of `target` through another sheet's formula (not by a `Connect` row, which names no row).
    fn readers_into(&self, target: &SheetRef) -> Vec<SheetRef> {
        let mut out: Vec<SheetRef> = Vec::new();
        for (key, entries) in &self.cross {
            if key.0 == *target {
                out.extend(entries.iter().filter(|e| e.phase() == 0).map(|e| e.reader.0.clone()));
            }
        }
        out
    }
}

/// A shape an edit replaced, with a sheet that is another allocation than before.
pub(crate) struct Replaced {
    pub(crate) sheet: SheetRef,
    pub(crate) was: Arc<Sheet>,
    pub(crate) now: Arc<Sheet>,
}

/// Whether two shapes are the same shape in the same place as far as what a reader of its sheet reads goes: its `ID` and names (a reference finds a sheet by
/// them), its kind, and its links to a master (the formulas it inherits).
fn same_identity(a: &Shape, b: &Shape) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.name_u == b.name_u
        && a.kind == b.kind
        && a.master == b.master
        && a.master_shape == b.master_shape
        && a.del == b.del
}

/// Finds the shapes `new` holds that `old` held in the same places, as other allocations, with their sheets as other allocations: appended to `out`, in the
/// order of a walk of the page. Whole lists and whole chunks and shapes that are the same allocation are skipped, so the cost is what the edit replaced.
/// Each shape looked at is a step. `false` when the two are not the same shapes in the same places (a length that differs, an `ID`, a name or a master
/// that differs, a tree deeper than the walk goes, or the steps ran out).
pub(crate) fn replaced_in(
    old: &ShapeList,
    new: &ShapeList,
    owner: Owner,
    path: &mut Vec<u32>,
    out: &mut Vec<Replaced>,
    ev: &mut Evaluator<'_, '_>,
) -> bool {
    if old.is_same_list_as(new) {
        return true;
    }
    if old.len() != new.len() {
        return false;
    }
    let mut base = 0_usize;
    for (a_chunk, b_chunk) in old.chunk_arcs().iter().zip(new.chunk_arcs()) {
        if a_chunk.len() != b_chunk.len() {
            return false;
        }
        if !Arc::ptr_eq(a_chunk, b_chunk) {
            for (k, (a, b)) in a_chunk.iter().zip(b_chunk.iter()).enumerate() {
                if Arc::ptr_eq(a, b) {
                    continue;
                }
                if !ev.steps(1) || !same_identity(a, b) {
                    return false;
                }
                let Ok(place) = u32::try_from(base.saturating_add(k)) else { return false };
                path.push(place);
                if !Arc::ptr_eq(&a.sheet, &b.sheet) {
                    out.push(Replaced {
                        sheet: SheetRef::Shape(ShapeRef { owner, path: path.clone() }),
                        was: Arc::clone(&a.sheet),
                        now: Arc::clone(&b.sheet),
                    });
                }
                let within = a.children.is_same_list_as(&b.children)
                    || (path.len() < MAX_SHAPE_DEPTH && replaced_in(&a.children, &b.children, owner, path, out, ev));
                path.pop();
                if !within {
                    return false;
                }
            }
        }
        base = base.saturating_add(a_chunk.len());
    }
    true
}

/// Whether the formulas two sheets hold are the same formulas in the same places: what the graph reads of a sheet. Each formula that is not the same
/// allocation is compared by its text, and charged by the length of it. **A section behind the same pointer in both is the same** and is not walked
/// (an edit that writes a shape's cells leaves its sections shared with the version before, and a move of ten thousand connectors walked the
/// route of each, thousands of rows of constants, twice): with as many sections in both, the cells directly under the sheets are compared, then each section
/// that is not shared with the one in its place. (Sheets that are the same part by part are the same; a difference found part by part where the whole lists
/// would agree, a formula moved between two sections of one key, only makes the sheet classified again.)
pub(crate) fn same_formulas(ev: &mut Evaluator<'_, '_>, was: &Sheet, now: &Sheet) -> Option<bool> {
    // **The links to the styles are what a sheet's inherited formulas are found by**: a sheet given another style reads what that style's formulas read.
    if was.links != now.links {
        return Some(false);
    }
    if was.sections.len() != now.sections.len() {
        return same_formula_lists(ev, local_formulas(was, None), local_formulas(now, None));
    }
    if !same_formula_lists(ev, local_formulas(was, Some(None)), local_formulas(now, Some(None)))? {
        return Some(false);
    }
    for (at, (a, b)) in was.sections.iter().zip(now.sections.iter()).enumerate() {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if !same_formula_lists(ev, local_formulas(was, Some(Some(at))), local_formulas(now, Some(Some(at))))? {
            return Some(false);
        }
    }
    Some(true)
}

/// The cells of `sheet` that hold a formula, with it, in the order a walk of the sheet meets them: all of them (`None`), or those of one part: directly under the
/// sheet (`Some(None)`), or of the section in place `n` (`Some(Some(n))`). With **how many cells hold `Inh`** (the formula they take is the chain's: the cell that turns into one, or out of
/// one, changes what the sheet reads), which costs no step.
fn local_formulas<'a>(sheet: &'a Sheet, part: Option<Option<usize>>) -> Formulas<'a> {
    let mut out: Vec<(CellAddr, &'a Arc<diacraft_doc::Formula>)> = Vec::new();
    let mut inherit = 0_usize;
    let mut take = |addr: CellAddr, cell: &'a diacraft_doc::Cell| match &cell.formula {
        FormulaSlot::Local(formula) => out.push((addr, formula)),
        FormulaSlot::Inherit => inherit = inherit.saturating_add(1),
        _ => {}
    };
    match part {
        None => {
            each_own_cell(sheet, &mut take);
            for section in &sheet.sections {
                each_section_cell(section, &mut take);
            }
        }
        Some(None) => each_own_cell(sheet, &mut take),
        Some(Some(n)) => {
            if let Some(section) = sheet.sections.get(n) {
                each_section_cell(section, &mut take);
            }
        }
    }
    Formulas { list: out, inherit }
}

/// What [`local_formulas`] found: the cells that hold a formula, and the number that hold `Inh`.
struct Formulas<'a> {
    list: Vec<(CellAddr, &'a Arc<diacraft_doc::Formula>)>,
    inherit: usize,
}

/// Whether two lists of formulas, by their places, are the same: the comparison [`same_formulas`] makes, of the whole sheets or of a part of each.
fn same_formula_lists(ev: &mut Evaluator<'_, '_>, before: Formulas<'_>, after: Formulas<'_>) -> Option<bool> {
    // Each formula looked at is a step, as asking it whether it reads beyond its sheet was.
    if !ev.steps(before.list.len().saturating_add(after.list.len())) {
        return None;
    }
    if before.list.len() != after.list.len() || before.inherit != after.inherit {
        return Some(false);
    }
    for ((a_addr, a), (b_addr, b)) in before.list.iter().zip(after.list.iter()) {
        if a_addr != b_addr {
            return Some(false);
        }
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if !ev.work(a.source.len()) {
            return None;
        }
        if a != b {
            return Some(false);
        }
    }
    Some(true)
}

/// Whether two sheets hold the same sections and rows: the same keys in the same places, none deleted or retyped that was not. (What a reference to a row of
/// the sheet is spelled as depends on them, not on the cells in them.)
fn same_rows(was: &Sheet, now: &Sheet) -> bool {
    if was.sections.len() != now.sections.len() {
        return false;
    }
    was.sections.iter().zip(now.sections.iter()).all(|(a, b)| {
        Arc::ptr_eq(a, b)
            || (a.name == b.name
                && a.ix == b.ix
                && a.del == b.del
                && a.rows.len() == b.rows.len()
                && a.rows.iter().zip(b.rows.iter()).all(|(x, y)| {
                    Arc::ptr_eq(x, y)
                        || (x.name == y.name && x.ix == y.ix && x.del == y.del && x.row_type == y.row_type && x.local_name == y.local_name)
                }))
    })
}

/// Whether two lists of one kind of part are the same list or hold the same parts.
fn same_list<T: PartialEq>(a: &SharedVec<Arc<T>>, b: &SharedVec<Arc<T>>) -> bool {
    a.is_same_list_as(b) || a == b
}

/// Whether two lists of pages hold the same pages as far as a reference to a page goes: the `ID` and the names, in the same places. The pages' content
/// is not read (the page the graph is for is held beside).
fn same_pages(a: &SharedVec<Arc<Page>>, b: &SharedVec<Arc<Page>>) -> bool {
    if a.is_same_list_as(b) {
        return true;
    }
    if a.len() != b.len() {
        return false;
    }
    a.chunk_arcs().iter().zip(b.chunk_arcs()).all(|(x, y)| {
        Arc::ptr_eq(x, y)
            || (x.len() == y.len()
                && x.iter().zip(y.iter()).all(|(p, q)| Arc::ptr_eq(p, q) || (p.id == q.id && p.name == q.name && p.name_u == q.name_u)))
    })
}

/// A page's graph, with what it was made for.
#[derive(Debug)]
struct Built {
    /// The page the graph describes, held, so that a pointer to it is none other's.
    page: Arc<Page>,
    /// What the graph reads of the rest of the document.
    masters: SharedVec<Arc<Master>>,
    styles: SharedVec<Arc<Style>>,
    pages: SharedVec<Arc<Page>>,
    graph: Graph,
}

impl Built {
    fn same_environment(&self, doc: &Document) -> bool {
        same_list(&self.masters, &doc.masters) && same_list(&self.styles, &doc.styles) && same_pages(&self.pages, &doc.pages)
    }

    /// The graph of `page`, made by a walk of the page. `None` when the budget ran out on the way.
    fn make(ev: &mut Evaluator<'_, '_>, doc: &Document, page: &Arc<Page>) -> Option<Built> {
        let mut built = Built {
            page: Arc::clone(page),
            masters: doc.masters.clone(),
            styles: doc.styles.clone(),
            pages: doc.pages.clone(),
            graph: Graph::default(),
        };
        let mut memo = MasterCross::default();
        let mut formulas = SectionFormulas::default();
        walk_page(doc, page.id, &mut |sheet, data| {
            let mut found = Found::default();
            classify_sheet(ev, sheet, data, &mut memo, &mut formulas, &mut found);
            built.graph.install(sheet, found);
        });
        built.graph.install_connects(connect_entries(ev, page));
        (!ev.ctx.stats.budget_exhausted).then_some(built)
    }

    /// Brings the graph up to date with `page`. `false` when it is to be made again: a change it does not follow, or the budget ran out (what was done to
    /// the graph is then of no use, and it is let go of by the caller).
    fn bring_up_to_date(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document, page: &Arc<Page>) -> bool {
        if Arc::ptr_eq(&self.page, page) && self.same_environment(doc) {
            #[cfg(test)]
            tally::bump(&tally::SAME_PAGE);
            return true;
        }
        if !self.same_environment(doc) || page.connects != self.page.connects {
            return false;
        }
        let owner = Owner::Page(page.id);
        let mut replaced: Vec<Replaced> = Vec::new();
        if !Arc::ptr_eq(&self.page.sheet, &page.sheet) {
            replaced.push(Replaced { sheet: SheetRef::Page(page.id), was: Arc::clone(&self.page.sheet), now: Arc::clone(&page.sheet) });
        }
        if !replaced_in(&self.page.shapes, &page.shapes, owner, &mut Vec::new(), &mut replaced, ev) {
            return false;
        }
        // The sheets whose entries are made again, once each.
        let mut again: Vec<SheetRef> = Vec::new();
        let mut known: HashSet<SheetRef> = HashSet::new();
        let mut note = |sheet: SheetRef, again: &mut Vec<SheetRef>| {
            if known.insert(sheet.clone()) {
                again.push(sheet);
            }
        };
        for r in &replaced {
            if !same_rows(&r.was, &r.now) {
                #[cfg(test)]
                tally::bump(&tally::ROWS_CHANGED);
                for reader in self.graph.readers_into(&r.sheet) {
                    note(reader, &mut again);
                }
            }
            match same_formulas(ev, &r.was, &r.now) {
                Some(true) => {
                    #[cfg(test)]
                    tally::bump(&tally::VALUES_ONLY);
                }
                Some(false) => note(r.sheet.clone(), &mut again),
                None => return false,
            }
        }
        let mut memo = MasterCross::default();
        let mut formulas = SectionFormulas::default();
        for sheet in &again {
            #[cfg(test)]
            tally::bump(&tally::RECLASSIFIED);
            let released = self.graph.remove(sheet);
            ev.ctx.release_bytes(released);
            let Some(data) = ev.res.sheet(sheet) else { continue };
            let mut found = Found::default();
            classify_sheet(ev, sheet, data, &mut memo, &mut formulas, &mut found);
            self.graph.install(sheet, found);
        }
        self.page = Arc::clone(page);
        self.masters = doc.masters.clone();
        self.styles = doc.styles.clone();
        self.pages = doc.pages.clone();
        #[cfg(test)]
        tally::bump(&tally::PUT_RIGHT);
        !ev.ctx.stats.budget_exhausted
    }
}

/// The entries the `Connect` rows of `page` make: the end cells of each glued connector read the pose of the shape it is glued to.
fn connect_entries(ev: &mut Evaluator<'_, '_>, page: &Page) -> Found {
    let mut found = Found::default();
    let owner = Owner::Page(page.id);
    let mut seq = 0_u32;
    for connect in page.connects.iter() {
        let Some(from_cell) = connect.from_cell.as_deref() else { continue };
        if !ev.steps(1) {
            return found;
        }
        let (Some(from), Some(to)) = (ev.shape_by_id(owner, connect.from_sheet), ev.shape_by_id(owner, connect.to_sheet)) else { continue };
        let to_keys = pose_keys(&SheetRef::Shape(to));
        for name in end_cells(from_cell) {
            let reader: Reader = (SheetRef::Shape(from.clone()), CellAddr { section: None, row: None, cell: CellName::new(name) });
            for key in &to_keys {
                if !found.charge(ev, 2 * KEY_BYTES) {
                    return found;
                }
                found.cross.push((key.clone(), reader.clone(), connect_ord(seq)));
            }
            seq = seq.saturating_add(1);
        }
    }
    found
}

/// The most pages whose graphs are kept at once beside the one an edit is on: the pages a change reached through a `Pages[..]` reference, which a later edit reaches again.
pub const MAX_KEPT_PAGES: usize = 256;

/// The readers of the cells of the pages an edit reaches, kept from one edit to the next: see the module documentation. A new one holds nothing and the first commit
/// that is given it makes the graph.
///
/// **One page's graph is the rule and others are the exception**: the page an edit is on has its graph made and kept, as it always had; a page that a change
/// reaches through a formula that names it (`pages.rs`) has its own made when the change first reaches a cell of it, and kept in step with the edits since the same way. They are let go of
/// together past [`MAX_KEPT_PAGES`] or [`MAX_KEPT_BYTES`], the page of the edit last, so a document with no formula that names a page keeps what it always did.
#[derive(Debug, Default)]
pub struct ReaderIndex {
    built: HashMap<u32, Built>,
    /// The page the last commit was for: its graph is the last to go.
    primary: Option<u32>,
    /// The sheets that name a page, for every page (`pages.rs`).
    pub(crate) links: PageLinks,
    /// What [`crate::changes_between_with`] remembers of the page's shapes between one call and the next.
    known: KnownShapes,
}

impl ReaderIndex {
    /// An index that holds nothing.
    pub fn new() -> ReaderIndex {
        ReaderIndex::default()
    }

    /// What the graphs kept are charged, as accounted (`0` when none is kept): what a session counts of them, and what a commit that uses one is charged first.
    pub fn held_bytes(&self) -> u64 {
        self.built.values().map(|b| b.graph.bytes).fold(self.links.bytes, u64::saturating_add)
    }

    /// Lets the graphs go, and what is remembered of the page's shapes.
    pub fn clear(&mut self) {
        self.built.clear();
        self.primary = None;
        self.links.clear();
        self.known = KnownShapes::default();
    }

    /// What `changes_between_with` remembers of the page's shapes.
    pub(crate) fn known_shapes(&mut self) -> &mut KnownShapes {
        &mut self.known
    }

    /// Brings the graph to the state of page `page_id` of `doc` and gives it, made again whole if it was not for this page or changed in a way it does not
    /// follow. The graph is charged to the commit's budget (what it held, then what the work of this edit adds). `None`, and no graph kept, when the budget
    /// ran out or the page is not there. This is the page the commit is for.
    pub(crate) fn sync(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document, page_id: u32) -> Option<&Graph> {
        self.primary = Some(page_id);
        self.sync_page(ev, doc, page_id);
        self.graph_of(page_id)
    }

    /// The graph of `page_id` as it was last brought up to date, if one is kept.
    pub(crate) fn graph_of(&self, page_id: u32) -> Option<&Graph> {
        self.built.get(&page_id).map(|b| &b.graph)
    }

    /// Brings the listing of the sheets that name a page to the state of `doc` (`pages.rs`).
    pub(crate) fn sync_links(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document) {
        self.links.sync(ev, doc, false);
    }

    /// [`ReaderIndex::sync_links`] for a change to the pages themselves: the sheets of a drawing of one page are listed too, since a page deleted down to one leaves cells that name a page
    /// that is not there.
    pub(crate) fn sync_links_for_pages(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document) {
        self.links.sync(ev, doc, true);
    }

    /// [`ReaderIndex::sync`] for any page: whether its graph is kept, up to date.
    pub(crate) fn sync_page(&mut self, ev: &mut Evaluator<'_, '_>, doc: &Document, page_id: u32) -> bool {
        let kept = self.built.remove(&page_id);
        let Some(page) = doc.pages.iter().find(|p| p.id == page_id).map(Arc::clone) else { return false };
        let mut built: Option<Built> = None;
        if let Some(mut old) = kept {
            if !ev.ctx.hold_bytes(old.graph.bytes) {
                return false;
            }
            if old.bring_up_to_date(ev, doc, &page) {
                built = Some(old);
            } else {
                // Given up: what the commit was charged for the graph is given back, and the graph that is made again is charged afresh.
                ev.ctx.release_bytes(old.graph.bytes);
                #[cfg(test)]
                tally::bump(&tally::GIVEN_UP);
            }
        }
        if built.is_none() && !ev.ctx.stats.budget_exhausted {
            #[cfg(test)]
            tally::bump(&tally::MADE);
            built = Built::make(ev, doc, &page);
        }
        match built {
            Some(kept) => {
                self.built.insert(page_id, kept);
                true
            }
            None => false,
        }
    }

    /// Ends a commit: graphs that are more than [`MAX_KEPT_BYTES`] or [`MAX_KEPT_PAGES`] are let go of.
    pub(crate) fn finish(&mut self) {
        self.finish_within(MAX_KEPT_BYTES);
    }

    /// [`ReaderIndex::finish`] for graphs that may be `most` bytes: the pages other than the commit's go first, then its own.
    pub(crate) fn finish_within(&mut self, most: u64) {
        if self.built.len() > MAX_KEPT_PAGES || self.held_bytes() > most {
            let primary = self.primary;
            self.built.retain(|id, _| Some(*id) == primary);
        }
        if self.held_bytes() > most {
            self.built.clear();
            self.links.clear();
        }
    }
}
