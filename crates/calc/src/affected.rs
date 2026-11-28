//! Which cells an edit can have changed: the cells it wrote and every cell that reads one of them, again and again until nothing more does
//! **A cell passes the change on only if it changed** (of the M3 review): the cells that read a cell the edit
//! wrote are worked out, and the cells that read one of those only if working it out planned a write ([`Affected::changed`]).
//!
//! **Why.** A command that moves a shape used to work out every formula on the page and write the result of each that differed from the saved
//! `V`. On a file from another application that rewrites cells nobody touched wherever this evaluator and the other application disagree, and the
//! writer then saves them. Now a command works out the cells it changed, and the cells that read them, and no others
//! ([`crate::commit_changed`]); a cell not on that list keeps its saved value, byte for byte. A cell worked out that comes out as it was (a
//! result that agrees with the saved value) or that this evaluator cannot work out passes nothing on: the cells that read only it are not on the
//! list, since nothing they read changed, though this evaluator might give them another result than the file's (a width a rounding away from the
//! one the file saved, from the two ends that did not move).
//!
//! **When the walk is by the whole page instead.** When `changes_between` reports [`Changes::whole_page`] (a style, a theme, the colours, the
//! document's sheet or a master changed, or a tree too large to index), every formula of the page is worked out, which writes more, and does so
//! without a word; the cells the page's instances inherit and their masters' cells are worked out where the edit changed them, measured from
//! the document it started from ([`Changes::since`], `commit.rs`). A page in which two shapes of one list hold one `ID` is compared shape by
//! shape, in their order.
//!
//! **What reads what.** A cell reads another when
//!
//! * its formula names it: unqualified (`Width`, `Geometry1.X2`: a cell of the cell's own sheet), or qualified (`Sheet.12!PinX`, `ThePage!User.Row`),
//!   found as the evaluator finds it ([`Evaluator::locate`]), so a name in a formula found on a master is read for the instance
//!   A row is told by its key; where a section's rows may be keyed by name or by place the key is dropped, and a name
//!   that reaches any row of that section reads every row of it: more cells are worked out than needed and none fewer. **A reference that is the argument
//!   of `GETREF` or `GETVAL` is not one of these** (it names a cell and is no link): a change to the
//!   cell it names does not work the formula out again, so a person's write to the target of a `SETF` does not make the call write its own text over it
//!   ([`crate::facts::Facts::links`] leaves it out, and [`crate::facts::Facts::named`], which orders the evaluation, keeps it);
//! * its formula calls a function that carries a point or an angle from one frame to another (`PAR`, `LOC`, `LOCTOLOC`, `LOCTOPAR`,
//!   `ANGLETOLOC`, `ANGLETOPAR`), which reads the pose cells (`PinX`, `PinY`, `LocPinX`, `LocPinY`, `Angle`, `FlipX`, `FlipY`) of its own shape and
//!   its groups, and of every shape its references name and their groups. This is how a connector glued by `PAR(PNT(Sheet.4!Connections.X1, ...))`
//!   follows the shape it is glued to;
//! * a `Connect` row says its end is glued to a shape: the end's cells read the pose of that shape, whatever their formulas are;
//! * a section was **written whole** ([`Changes::sections`]: a route replaces the rows of its connector's first `Geometry` section): every cell that reads any cell of it, by its
//!   own name or through a function that reads the whole section, reads the change. A reference to a `Geometry` cell is listed under the section's own place as well as the cell's
//!   (`written_whole`), so that the change finds it whichever cell it names, the ones the rows lost and the ones they gained included; no other section is listed twice;
//! * it inherits a formula that reads the cell: an instance's `Inh` cell, or a cell it does not hold at all and gets from its master, with the
//!   master's references read on the instance, and a cell a sheet holds as `Inh` whose formula the chain finds on a style, read on the sheet itself
//!   ([`crate::pages::style_given`]: asked only of a sheet whose links name a style that holds a formula that reads beyond its sheet);
//! * it names a sheet no page holds (`TheDoc!`, `Styles!`, `Masters[..]!`), found for any page through the listing of `pages.rs` when the cell it names is one a commit worked out.
//!
//! **What is always worked out.** A cell that reads what its text does not name: a reference with a call before it (`LISTSHEETREF()!Width`),
//! or a function that reads the members of a list, a container or a callout, or a string as a formula ([`crate::facts::Facts::dynamic`]), is
//! worked out after every edit that changed anything, and so is a cell with a reference that finds no sheet after an edit that took a shape out
//! it may have named: a plain `Sheet.<id>` of the part only when the shape of that `ID` was one of those taken out (one that names another found nothing
//! before the edit either, and keeps the value the file saved), any other spelling after any such edit. **What is not followed.** What `THEMEVAL`, `TEXTWIDTH` and the like read of the sheet's theme cells and text: the
//! editing commands change none of those, and a caller whose edit does sets [`Changes::whole_page`].
//!
//! **What it costs.** The references of a formula are found once for each formula (they are kept under the formula's address, `facts.rs`); the
//! cells of a sheet that read one of its own cells are found when a change first reaches the sheet, once for a sheet and once for a master shape,
//! whatever the number of its instances. The references to other sheets, the frame functions and the `Connect` rows are listed for the page **once,
//! and kept from one edit to the next** ([`crate::ReaderIndex`], `index.rs`): an edit puts right what it read of the sheets it replaced, and no
//! more, so what an edit costs is what it touched and not the formulas of the page (a move on a page of 12,000 shapes asked 168,000
//! formulas whether they read beyond their sheet, and made a vector for each, before this). Each step is charged to the budget.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_doc::{
    CellAddr, CellName, Document, Formula, FormulaSlot, Keying, Owner, Rebind, RowKey, SectionKey, ShapeRef, Sheet, SheetRef, keying,
};

use crate::changes::{Changes, SectionFormulas, each_cell, each_formula};
use crate::eval::{Env, Evaluator};
use crate::facts::{PATH_FUNCTIONS, TEXT_MEASURES, TextRead, reach};
use crate::index::{Graph, ReaderIndex, walk_ord};
use crate::pages::{CrossMap, Target};
use crate::refs::address_of;
use crate::rules::KEY_BYTES;

#[cfg(test)]
mod oracle;
#[cfg(test)]
mod tests;

/// Where a cell is in its sheet: the section, the row, and the cell's name. The row is left out for a section whose rows may be keyed by name or
/// by place (`Keying::Either`), where a reference and a row might name one row two ways: a name that reaches a row of such a section reads every
/// row of it, so more cells are worked out than need be and none fewer. In a section keyed by name (`User`, `Prop`) or by place (`Geometry`) a
/// row is told by its key, which a reference and the sheet spell alike.
pub(crate) type KeyCell = (Option<SectionKey>, Option<RowKey>, CellName);
/// A cell as something is read through: its sheet and its place.
pub(crate) type Key = (SheetRef, KeyCell);
/// A cell that reads: its sheet and its exact address.
pub(crate) type Reader = (SheetRef, CellAddr);

/// The cells that set a shape's frame: what a function that carries a point between frames reads.
const POSE: [&str; 7] = ["PinX", "PinY", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];

/// The name the seven pose cells of a shape are read under together: the readers of a frame are listed once for a shape, under this, and not
/// seven times. No cell has it.
const POSE_GROUP: &str = "\u{0}pose";

/// The name a wildcard is kept under in the place of a cell: "any cell of this section". No cell has it.
const ANY_CELL: &str = "\u{0}any";

/// The name a reader of one cell of a section is kept under besides the cell: "some cell of this section is read here". No cell has it.
const SOME_CELL: &str = "\u{0}some";

/// The place that stands for every cell of a section (`Geometry1.*`, `Char.*`): a function that reads a whole section is listed under it, and a
/// change to any cell of the section finds it.
fn any_of(section: &SectionKey) -> KeyCell {
    (Some(section.clone()), None, CellName::new(ANY_CELL))
}

/// The place a reader of one cell of a section is also listed under, for a change that is to the section as a whole ([`Changes::sections`]): "some cell of this section is read
/// here". A change to the place finds it, and finds what [`any_of`] holds as well.
fn some_of(section: &SectionKey) -> KeyCell {
    (Some(section.clone()), None, CellName::new(SOME_CELL))
}

/// The place a reader of the cell `named` is listed under besides the cell, when the cell is one of a section whose rows a command writes whole: a `Geometry` section, which a route
/// replaces (`Changes::sections`). Not every section: a reference to a cell of `Connections` or `User` is found by the cell, and listing each twice would double what a page of
/// glued connectors holds (every glued end reads `Sheet.<id>!Connections.X1`).
fn written_whole(named: &CellAddr) -> Option<KeyCell> {
    match &named.section {
        Some(section) if section.name.as_str() == "Geometry" => Some(some_of(section)),
        _ => None,
    }
}

/// A cell directly under a sheet, as a place.
fn direct_place(name: &str) -> KeyCell {
    (None, None, CellName::new(name))
}

/// What a path function reads that its text does not name, given the cell its first argument names: every cell of that `Geometry` section, and
/// the size and rounding of the shape that holds it.
fn path_reads(named: &CellAddr) -> Vec<KeyCell> {
    match &named.section {
        Some(section) if section.name.as_str() == "Geometry" => {
            vec![any_of(section), direct_place("Width"), direct_place("Height"), direct_place("Rounding")]
        }
        _ => Vec::new(),
    }
}

/// What `TEXTWIDTH` and `TEXTHEIGHT` read of the sheet that holds the text, and its text does not name: the formats of its runs, its tab stops and
/// its margins (`fn_measure.rs`).
fn text_reads() -> Vec<KeyCell> {
    let mut reads: Vec<KeyCell> = ["Character", "Paragraph", "Tabs"].into_iter().map(|s| any_of(&SectionKey::new(s, None))).collect();
    reads.extend(
        ["LeftMargin", "RightMargin", "TopMargin", "BottomMargin", "VerticalAlign", "TextDirection", "DefaultTabStop"].into_iter().map(direct_place),
    );
    reads
}

/// The sheet of the page or master part a shape is on: its scale is read by the text measures.
fn part_sheet(sheet: &SheetRef) -> Option<SheetRef> {
    match sheet {
        SheetRef::Shape(at) => Some(match at.owner {
            Owner::Page(id) => SheetRef::Page(id),
            Owner::Master(id) => SheetRef::Master(id),
        }),
        _ => None,
    }
}

/// Whether a formula calls one of `names`.
fn calls_any(functions: &[String], names: &[&str]) -> bool {
    functions.iter().any(|f| names.contains(&f.as_str()))
}

fn place_of(addr: &CellAddr) -> KeyCell {
    let either = addr.section.as_ref().is_some_and(|s| matches!(keying(s.name.as_str()), Keying::Either));
    // A row named in another letter case is the same row, so a reader of `User.tally9` reads the cell of `Tally9`.
    let row = match &addr.row {
        _ if either => None,
        Some(RowKey::Name(name)) if name.chars().any(char::is_uppercase) => Some(RowKey::Name(crate::row_names::fold(name).into())),
        other => other.clone(),
    };
    (addr.section.clone(), row, addr.cell.clone())
}

fn key_of(sheet: &SheetRef, addr: &CellAddr) -> Key {
    (sheet.clone(), place_of(addr))
}

/// The shape and the groups it is in, outermost last: the frames a point passes through on its way to the page.
fn chain(sheet: &SheetRef) -> Vec<SheetRef> {
    match sheet {
        SheetRef::Shape(at) => (1..=at.path.len())
            .filter_map(|k| at.path.get(..k).map(|prefix| SheetRef::Shape(ShapeRef { owner: at.owner, path: prefix.to_vec() })))
            .collect(),
        _ => Vec::new(),
    }
}

/// The group a shape is in, if it is in one.
fn parent_of(sheet: &SheetRef) -> Option<SheetRef> {
    match sheet {
        SheetRef::Shape(at) => at.parent().map(SheetRef::Shape),
        _ => None,
    }
}

/// What a reader of the frames a shape is carried through is listed under: one key for the shape and for each group it is in.
pub(crate) fn pose_keys(sheet: &SheetRef) -> Vec<Key> {
    chain(sheet).into_iter().map(|s| (s, (None, None, CellName::new(POSE_GROUP)))).collect()
}

/// Cells of a sheet with the formulas they hold.
type Formulas = Vec<(CellAddr, Arc<Formula>)>;

/// The formulas of master shapes that read beyond their sheet, by master shape: found once for each, whatever the number of its instances.
pub(crate) type MasterCross = HashMap<SheetRef, Arc<Formulas>>;

/// The cells of one sheet that read a cell of the same sheet, by the cell they read.
type Intra = Rc<HashMap<KeyCell, Vec<CellAddr>>>;

/// What the classification of one sheet found: the entries it puts in the graph (each with where a walk of the page finds it, which orders the readers
/// of a cell), and what they are charged to the budget, as accounted.
#[derive(Debug, Default)]
pub(crate) struct Found {
    /// The readers of a cell through a qualified reference, a frame function or a `Connect` row.
    pub(crate) cross: Vec<(Key, Reader, u64)>,
    /// The readers of cells their text does not name.
    pub(crate) dynamic: Vec<(Reader, u64)>,
    /// The readers with a qualified reference that finds no sheet, each with the `ID` its reference names when it is a plain `Sheet.<id>` of the page
    /// (and `None` for any other spelling, which a shape taken out may have answered).
    pub(crate) dangling: Vec<(Reader, u64, Option<u32>)>,
    /// What the entries are charged.
    pub(crate) bytes: u64,
}

impl Found {
    /// Charges `n` bytes to the budget, to be held with what this found. `false`, and nothing charged, when the memory of the budget runs out.
    pub(crate) fn charge(&mut self, ev: &mut Evaluator<'_, '_>, n: u64) -> bool {
        if ev.ctx.hold_bytes(n) {
            self.bytes = self.bytes.saturating_add(n);
            true
        } else {
            false
        }
    }
}

/// The page a sheet is on: a page's own sheet, or a shape of its part. `None` for the sheets no page holds (the document's, a style's, a master's and its shapes').
pub(crate) fn page_of(sheet: &SheetRef) -> Option<u32> {
    match sheet {
        SheetRef::Page(id) => Some(*id),
        SheetRef::Shape(at) => match at.owner {
            Owner::Page(id) => Some(id),
            Owner::Master(_) => None,
        },
        SheetRef::Document | SheetRef::Style(_) | SheetRef::Master(_) => None,
    }
}

/// The cells to work out for an edit, found as they are worked out (of the M3 review). The edit's own cells ([`Changes`]) and the cells
/// that are always worked out are there from the start; **the readers of a cell are noted when the cell changed**: for a cell the edit wrote, or
/// that is new, at once; for any other, when working it out ([`crate::commit_changed`]) planned a write ([`Affected::changed`]). A reader that comes
/// out as it was ends the chain, and so does a cell this evaluator cannot work out (a function it does not know): what reads those keeps the
/// value the file saved, whatever this evaluator would give for it. A connector end glued by a function that `calc` cannot work out, and
/// so left as it was, does not pass a move on to the width and the pin that read it, which would come out a rounding away from the file's own.
///
/// **Across pages** (F40): the cells that read a cell of another page through `Pages[..]` are readers of it as any are
/// ([`Affected::readers_of`] asks `pages.rs` which sheets name the page and [`CrossMap`] says what they read of it), and a cell of another page that changed passes the change on through
/// that page's own graph, kept in the index beside the page the commit is for. The chain goes on from page to page in the one commit, on the one evaluator.
pub(crate) struct Affected<'i> {
    /// The graph of the commit's page and of the pages a change has reached, and the listing of the sheets that name a page.
    index: &'i mut ReaderIndex,
    /// The page the commit is for: its graph also answers the cells no page holds (a master's, a style's, the document's).
    page: u32,
    /// The pages whose graph is up to date for this commit.
    ready: HashSet<u32>,
    /// What this commit finds as it goes and does not keep.
    scratch: Scratch,
    /// Every cell noted, once.
    seen: HashSet<Reader>,
    /// The cells noted and not yet handed out, in the order they were found.
    todo: VecDeque<Reader>,
    /// The cells whose readers have been noted.
    followed: HashSet<Key>,
    /// The pages a change reached whose cells that read what their text does not name have been noted: a change to a page works them out, as it does for the commit's page.
    dynamic_noted: HashSet<u32>,
    /// Whether the cells of the sheets that read every page (a text worked out as a formula) have been noted: a change to a cell of any page works them out, once.
    wild_noted: bool,
}

/// What a commit finds out as it goes and lets go of at its end.
#[derive(Default)]
struct Scratch {
    /// The readers of a cell of their own sheet, for each sheet a change has reached.
    intra: HashMap<SheetRef, Intra>,
    /// The same for the formulas a master shape holds: one for all its instances.
    master_intra: HashMap<SheetRef, Intra>,
    /// The cells of the sections those sheets hold that hold a formula or inherit one, listed once for a section however many sheets share it.
    formulas: SectionFormulas,
    /// What the sheets that name a page, the document's sheet, the styles or the masters read of it, for each a change has reached (`None`: no sheet names it).
    cross: HashMap<Target, Option<Rc<CrossMap>>>,
    /// The cells of the sheets that read every page, which read what their text does not name: found when first asked.
    wild: Option<Rc<Vec<Reader>>>,
}

/// The readers of the cell `key` in a graph: through a qualified reference, a frame function or a `Connect` row, and the readers of the whole section it is in, and of the frame it is part of.
fn across_in(graph: Option<&Graph>, key: &Key, out: &mut Vec<Reader>) {
    let Some(graph) = graph else { return };
    let (sheet, place) = key;
    out.extend(graph.cross.get(key).into_iter().flatten().map(|e| e.reader.clone()));
    // A function that reads a whole section is listed under the section's wildcard, and reads every cell of it.
    if let Some(any) = place.0.as_ref().map(any_of) {
        out.extend(graph.cross.get(&(sheet.clone(), any)).into_iter().flatten().map(|e| e.reader.clone()));
    }
    if is_pose_cell(place) {
        let group = (sheet.clone(), (None, None, CellName::new(POSE_GROUP)));
        out.extend(graph.cross.get(&group).into_iter().flatten().map(|e| e.reader.clone()));
    }
}

/// [`across_in`] for what the sheets that name a page read of it.
fn across_map(map: &CrossMap, key: &Key, out: &mut Vec<Reader>) {
    let (sheet, place) = key;
    out.extend(map.cross.get(key).into_iter().flatten().cloned());
    if let Some(any) = place.0.as_ref().map(any_of) {
        out.extend(map.cross.get(&(sheet.clone(), any)).into_iter().flatten().cloned());
    }
    if is_pose_cell(place) {
        let group = (sheet.clone(), (None, None, CellName::new(POSE_GROUP)));
        out.extend(map.cross.get(&group).into_iter().flatten().cloned());
    }
}

/// Whether the place is one of the seven cells that set a shape's frame.
fn is_pose_cell(place: &KeyCell) -> bool {
    place.0.is_none() && place.1.is_none() && POSE.contains(&place.2.as_str())
}

impl<'i> Affected<'i> {
    /// The cells to start from for `changes` on page `page`: the cells changed, the cells of a shape that is new, the cells that read a shape
    /// that was taken out, and the cells that are always worked out. The readers of other sheets are those `index` keeps, brought up to date with
    /// the page.
    pub(crate) fn new(ev: &mut Evaluator<'_, '_>, doc: &Document, page: u32, changes: &Changes, index: &'i mut ReaderIndex) -> Affected<'i> {
        let kept = index.sync(ev, doc, page).is_some();
        index.sync_links(ev, doc);
        let mut ready: HashSet<u32> = HashSet::new();
        if kept {
            ready.insert(page);
        }
        let mut work = Affected {
            index,
            page,
            ready,
            scratch: Scratch::default(),
            seen: HashSet::new(),
            todo: VecDeque::new(),
            followed: HashSet::new(),
            dynamic_noted: HashSet::new(),
            wild_noted: false,
        };
        // What the edit changed is changed whatever working it out gives: its readers are noted below.
        let mut changed: Vec<Key> = Vec::with_capacity(changes.cells.len());
        for (sheet, addr) in &changes.cells {
            // A commit that is only for the readers of what was written (`Changes::only_if_read`) is told the cells hold their values: they are not worked out, and their readers are.
            if !changes.only_if_read {
                work.add(ev, (sheet.clone(), addr.clone()));
            }
            changed.push(key_of(sheet, addr));
        }
        // The cells to work out that nothing says changed (the cells that name the pages an edit gave another reading): their readers follow only if they are written.
        for (sheet, addr) in &changes.reread {
            work.add(ev, (sheet.clone(), addr.clone()));
        }
        // A row or a section that appeared, went or took another type changed what a function that reads the whole section reads, though no cell
        // of it was written: it counts as a change to "any cell" of the section.
        for (sheet, section, row) in &changes.rows {
            let any = CellAddr { section: Some(section.clone()), row: row.clone(), cell: CellName::new(ANY_CELL) };
            changed.push(key_of(sheet, &any));
        }
        // A section written whole changed every cell it holds, and the rows it held and has not: what reads one cell of it is found under the section's own place
        // (`written_whole`), and what reads the whole of it under `any_of`, which `readers_of` finds beside it.
        for (sheet, section) in &changes.sections {
            changed.push((sheet.clone(), some_of(section)));
        }
        for sheet in &changes.fresh {
            work.add_fresh(ev, sheet, &mut changed);
        }
        if kept {
            work.add_always(ev, changes);
        }
        for key in changed {
            work.follow(ev, key);
        }
        work
    }

    /// The cells that are worked out after any edit that changed something on the page: those whose reads their text does not name, and the cells with a reference that finds no sheet when a
    /// shape it may have named was taken out.
    fn add_always(&mut self, ev: &mut Evaluator<'_, '_>, changes: &Changes) {
        // **A reference that finds no sheet is worked out when a shape it may have named was taken out**: a `Sheet.<id>` of the page found nothing before
        // the edit unless the shape of that `ID` was one of those taken out, so one that names another `ID` is left with the value the file saved (writing its
        // error would change a cell the edit did not reach). Any other spelling (a name, a qualifier of a part, one found on a master) is worked out as before.
        let mut readers: Vec<Reader> = Vec::new();
        if let Some(graph) = self.index.graph_of(self.page) {
            if !changes.removed.is_empty() {
                let removed: HashSet<u32> = changes.removed.iter().copied().collect();
                readers.extend(graph.dangling.iter().filter(|e| e.named.is_none_or(|id| removed.contains(&id))).map(|e| e.reader.clone()));
            }
            if !changes.cells.is_empty()
                || !changes.rows.is_empty()
                || !changes.sections.is_empty()
                || !changes.fresh.is_empty()
                || !changes.removed.is_empty()
                || !changes.reread.is_empty()
            {
                readers.extend(graph.dynamic.iter().map(|e| e.reader.clone()));
            }
        }
        // The cells of other pages that named a shape that was taken out from this one read nothing now.
        if !changes.removed.is_empty()
            && let Some(map) = self.cross_map(ev, Target::Page(self.page))
        {
            readers.extend(map.dangling.iter().cloned());
        }
        for reader in readers {
            self.add(ev, reader);
        }
    }

    /// The next cell to work out, if any is left.
    pub(crate) fn next(&mut self) -> Option<Reader> {
        self.todo.pop_front()
    }

    /// Working out `cell` planned a write: it changed, and the cells that read it are to be worked out.
    pub(crate) fn changed(&mut self, ev: &mut Evaluator<'_, '_>, cell: &Reader) {
        self.follow(ev, key_of(&cell.0, &cell.1));
    }

    /// Notes a cell to work out, once. Each is held memory, and past the budget's the walk goes on finding nothing (the commit then ends as
    /// out of budget: what is gathered is still worked out).
    fn add(&mut self, ev: &mut Evaluator<'_, '_>, reader: Reader) {
        if !self.seen.contains(&reader) && ev.ctx.hold_bytes(3 * KEY_BYTES) {
            self.seen.insert(reader.clone());
            self.todo.push_back(reader);
        }
    }

    /// Notes the cells that read the cell `key`, once for a cell.
    fn follow(&mut self, ev: &mut Evaluator<'_, '_>, key: Key) {
        if !self.followed.insert(key.clone()) || !ev.steps(1) {
            return;
        }
        for reader in self.readers_of(ev, &key) {
            self.add(ev, reader);
        }
    }

    /// A shape that is new, or has another master: the formulas it inherits are worked out, and every cell its master holds counts as changed (a
    /// formula that names `Sheet.<id>!Width` of it reads one it did not hold before).
    fn add_fresh(&mut self, ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, changed: &mut Vec<Key>) {
        let SheetRef::Shape(at) = sheet else { return };
        let Ok(Some(step)) = ev.res.master_step(at) else { return };
        let Some(master) = ev.res.sheet(&SheetRef::Shape(step.shape)) else { return };
        each_cell(master, |addr, cell| {
            if matches!(cell.formula, FormulaSlot::Local(_)) {
                self.add(ev, (sheet.clone(), addr.clone()));
            }
            if ev.ctx.hold_bytes(KEY_BYTES) {
                changed.push(key_of(sheet, &addr));
            }
        });
    }

    /// The graph of `page` for this commit: brought up to date when first asked. Whether it is kept.
    fn reach(&mut self, ev: &mut Evaluator<'_, '_>, page: u32) -> bool {
        if self.ready.contains(&page) {
            return true;
        }
        let doc = ev.doc;
        let kept = self.index.sync_page(ev, doc, page);
        if kept {
            self.ready.insert(page);
        }
        kept
    }

    /// What the sheets that name `target` (a page, the document's sheet, the styles or the masters) read of it, worked out when first asked, for this commit. `None` when no sheet names it that
    /// the commit's own page's graph does not answer: the sheets of another page, and the sheets no page holds.
    fn cross_map(&mut self, ev: &mut Evaluator<'_, '_>, target: Target) -> Option<Rc<CrossMap>> {
        if let Some(known) = self.scratch.cross.get(&target) {
            return known.clone();
        }
        let sheets = match target {
            Target::Page(page) => ev.doc.page(page).map(|p| self.index.links.naming(p)).unwrap_or_default(),
            other => self.index.links.naming_unpaged(other, self.page),
        };
        let made = if sheets.is_empty() { None } else { Some(Rc::new(CrossMap::make(ev, &sheets, target, &mut self.scratch.formulas))) };
        self.scratch.cross.insert(target, made.clone());
        made
    }

    /// The cells of the sheets that read every page that read at run time, found when first asked for this commit.
    fn wild_readers(&mut self, ev: &mut Evaluator<'_, '_>) -> Rc<Vec<Reader>> {
        if let Some(known) = &self.scratch.wild {
            return Rc::clone(known);
        }
        let found = Rc::new(self.index.links.wild_readers(ev, &mut self.scratch.formulas));
        self.scratch.wild = Some(Rc::clone(&found));
        found
    }

    /// The cells that read the cell `key`: through other sheets, in its own sheet, and in an instance of its sheet's master shape.
    fn readers_of(&mut self, ev: &mut Evaluator<'_, '_>, key: &Key) -> Vec<Reader> {
        let mut out: Vec<Reader> = Vec::new();
        let (sheet, place) = key;
        // The readers through other sheets: those the commit's page holds, and, for a cell of another page, the page's own and those of the pages that name it. **A cell of a sheet no page holds**
        // (the document's, a style's, a master's and its shapes') is read by the sheets that name it on every page (`TheDoc!`, `Styles!`, `Masters[..]!`), which the listing says
        // The commit's page's graph answers its own.
        across_in(self.index.graph_of(self.page), key, &mut out);
        let target = Target::of(sheet);
        if let Target::Page(home) = target
            && home != self.page
            && self.reach(ev, home)
        {
            across_in(self.index.graph_of(home), key, &mut out);
            // A page a change has reached is a page that changed: its cells that read what their text does not name are worked out, as those of the commit's page are.
            if self.dynamic_noted.insert(home)
                && let Some(graph) = self.index.graph_of(home)
                && ev.steps(graph.dynamic.len())
            {
                out.extend(graph.dynamic.iter().map(|e| e.reader.clone()));
            }
        }
        if let Some(map) = self.cross_map(ev, target) {
            across_map(&map, key, &mut out);
        }
        // The cells that read at run time, in the sheets that read every page, are worked out after a change to a cell of any sheet, once.
        if !self.wild_noted {
            self.wild_noted = true;
            let wild = self.wild_readers(ev);
            if ev.steps(wild.len()) {
                out.extend(wild.iter().cloned());
            }
        }
        // A function that reads a whole section is listed under the section's wildcard, and reads every cell of it.
        let wildcard = place.0.as_ref().map(any_of);
        // The sheets no page holds too (the document's, a style's, a master's page sheet): a cell written there is read by the cells of its own sheet that name it,
        // and an edit that works them out (`sheet.set_cell`) follows them.
        if matches!(sheet, SheetRef::Shape(_) | SheetRef::Page(_) | SheetRef::Document | SheetRef::Style(_) | SheetRef::Master(_)) {
            let own = self.scratch.intra_of(ev, sheet);
            out.extend(own.get(place).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            if let Some(any) = &wildcard {
                out.extend(own.get(any).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            }
        }
        if let SheetRef::Shape(at) = sheet
            && matches!(at.owner, Owner::Page(_))
            && let Ok(Some(step)) = ev.res.master_step(at)
        {
            let inherited = self.scratch.master_intra_of(ev, &step.shape);
            out.extend(inherited.get(place).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            if let Some(any) = &wildcard {
                out.extend(inherited.get(any).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            }
        }
        out
    }
}

/// Whether a formula reads anything but unqualified cells of its own sheet ([`crate::facts::Reach`]); each node is a step.
pub(crate) fn reaches_beyond_own_sheet(ev: &mut Evaluator<'_, '_>, formula: &Formula) -> bool {
    crate::work::asked();
    let Ok(expr) = &formula.ast else { return false };
    let (found, nodes) = reach(expr);
    ev.steps(nodes) && found.beyond_own_sheet()
}

/// Finds what the formulas of the sheet `sheet` (whose cells are `data`) read beyond the sheet: those it holds, and those it inherits from its master
/// shape, which `memo` keeps for the master shape for the next instance. What is found is in `found`, each entry in the place a walk of the page
/// would list it.
/// The cells of the sections are listed once for each section in `formulas`, however many sheets share it.
pub(crate) fn classify_sheet(
    ev: &mut Evaluator<'_, '_>,
    sheet: &SheetRef,
    data: &Sheet,
    memo: &mut MasterCross,
    formulas: &mut SectionFormulas,
    found: &mut Found,
) {
    let own = Env::new(sheet.clone(), None);
    let mut ordinal = 0_u32;
    each_formula(data, formulas, |addr, formula| {
        if let Some(formula) = formula {
            classify(ev, found, sheet, addr, formula, &own, walk_ord(ordinal));
            ordinal = ordinal.saturating_add(1);
        }
    });
    // **The formulas the sheet gets from a style** for the cells it holds as `Inh`, worked out on the sheet itself: a sheet none of whose links names a style that holds a formula that reads
    // beyond its sheet is not asked for them.
    for (addr, formula) in crate::pages::style_given(ev, sheet, data, formulas) {
        classify(ev, found, sheet, &addr, &formula, &own, walk_ord(ordinal));
        ordinal = ordinal.saturating_add(1);
    }
    let SheetRef::Shape(at) = sheet else { return };
    let Ok(Some(step)) = ev.res.master_step(at) else { return };
    let inherited = master_cross_of(ev, memo, &step.shape);
    if inherited.is_empty() {
        return;
    }
    let env = Env::new(sheet.clone(), Some(Rebind { instance: step.instance, master: step.master }));
    for (addr, formula) in inherited.iter() {
        classify(ev, found, sheet, addr, formula, &env, walk_ord(ordinal));
        ordinal = ordinal.saturating_add(1);
    }
}

/// The formulas a master shape holds that read other sheets (or what their text does not name): found once for the shape.
fn master_cross_of(ev: &mut Evaluator<'_, '_>, memo: &mut MasterCross, shape: &ShapeRef) -> Arc<Formulas> {
    let sheet = SheetRef::Shape(shape.clone());
    if let Some(known) = memo.get(&sheet) {
        return Arc::clone(known);
    }
    let mut list: Formulas = Vec::new();
    if let Some(data) = ev.res.sheet(&sheet) {
        each_cell(data, |addr, cell| {
            if let FormulaSlot::Local(formula) = &cell.formula {
                list.push((addr, Arc::clone(formula)));
            }
        });
    }
    // Only the formulas that reach beyond their sheet are kept, each tested once for the master.
    list.retain(|(_, formula)| reaches_beyond_own_sheet(ev, formula));
    let list = Arc::new(list);
    memo.insert(sheet, Arc::clone(&list));
    list
}

/// The `ID` a reference names when it is a plain `Sheet.<id>` of the part its formula is in (`Sheet.12!Width`, no other qualifier, not found on a master): the one
/// spelling whose answer a shape taken out from the page decides by its `ID` alone. `None` for any other.
pub(crate) fn plain_sheet_id(r: &diacraft_formula::CellRef, env: &Env) -> Option<u32> {
    if env.rebind.is_some() {
        return None;
    }
    let [diacraft_formula::Qualifier::Path(segments)] = r.qualifiers.as_slice() else { return None };
    let [word, id] = segments.as_slice() else { return None };
    if !word.name.eq_ignore_ascii_case("Sheet") || word.index.is_some() || id.index.is_some() {
        return None;
    }
    id.name.parse().ok()
}

/// Records what the formula of the cell `addr` of `sheet` reads beyond the sheet's own cells, if anything: the `ord`-th formula of the sheet.
fn classify(ev: &mut Evaluator<'_, '_>, found: &mut Found, sheet: &SheetRef, addr: &CellAddr, formula: &Arc<Formula>, env: &Env, ord: u64) {
    // Most formulas name nothing but cells of their own sheet: they are told by a walk that allocates nothing, and their references are not listed.
    if !reaches_beyond_own_sheet(ev, formula) {
        return;
    }
    let Some(facts) = ev.facts_of(formula) else { return };
    let reader: Reader = (sheet.clone(), addr.clone());
    if facts.dynamic {
        found.dynamic.push((reader.clone(), ord));
    }
    // The shapes whose frames a function carries between: each one a reference names, the groups the cell's own shape is in, and the shape itself
    // where the formula reads its pose too (`Facts::frame_own`).
    let mut frames: Vec<SheetRef> = Vec::new();
    if facts.frame {
        frames.extend(if facts.frame_own { Some(sheet.clone()) } else { parent_of(sheet) });
    }
    let reads_path = calls_any(&facts.functions, &PATH_FUNCTIONS);
    for r in facts.links().filter(|r| !r.qualifiers.is_empty()) {
        if !ev.steps(1) || !found.charge(ev, 2 * KEY_BYTES) {
            return;
        }
        match ev.locate(r, env) {
            Ok((target, at)) => {
                found.cross.push((key_of(&target, &at), reader.clone(), ord));
                // A path function reads the whole section its first argument names, on the sheet the reference names.
                if reads_path {
                    for place in path_reads(&at) {
                        if !found.charge(ev, 2 * KEY_BYTES) {
                            return;
                        }
                        found.cross.push(((target.clone(), place), reader.clone(), ord));
                    }
                } else if let Some(place) = written_whole(&at) {
                    // A cell of a section a command writes whole is read by this formula too when the section is written (`Changes::sections`).
                    if !found.charge(ev, 2 * KEY_BYTES) {
                        return;
                    }
                    found.cross.push(((target.clone(), place), reader.clone(), ord));
                }
                if facts.frame {
                    frames.push(target);
                }
            }
            Err(_) => found.dangling.push((reader.clone(), ord, plain_sheet_id(r, env))),
        }
    }
    // A text measure reads the formats, tab stops and margins of the sheet that holds the text, and the scale of the page or master it is on.
    if calls_any(&facts.functions, &TEXT_MEASURES) {
        for read in &facts.text_reads {
            let holder = match read {
                TextRead::Own => Some(sheet.clone()),
                TextRead::Of(r) => ev.locate(r, env).ok().map(|(s, _)| s),
            };
            let Some(holder) = holder else { continue };
            let scale_sheet = part_sheet(&holder);
            for place in text_reads() {
                if !ev.steps(1) || !found.charge(ev, 2 * KEY_BYTES) {
                    return;
                }
                found.cross.push(((holder.clone(), place), reader.clone(), ord));
            }
            if let Some(page) = scale_sheet {
                for name in ["PageScale", "DrawingScale"] {
                    if !found.charge(ev, 2 * KEY_BYTES) {
                        return;
                    }
                    found.cross.push(((page.clone(), direct_place(name)), reader.clone(), ord));
                }
            }
        }
    }
    for frame in &frames {
        for key in pose_keys(frame) {
            if !found.charge(ev, 2 * KEY_BYTES) {
                return;
            }
            found.cross.push((key, reader.clone(), ord));
        }
    }
}

impl Scratch {
    /// The cells of `sheet` that read a cell of `sheet`, by the cell read: found when first asked.
    fn intra_of(&mut self, ev: &mut Evaluator<'_, '_>, sheet: &SheetRef) -> Intra {
        if let Some(known) = self.intra.get(sheet) {
            return Rc::clone(known);
        }
        let built: Intra = Rc::new(build_intra(ev, sheet, false, &mut self.formulas));
        self.intra.insert(sheet.clone(), Rc::clone(&built));
        built
    }

    /// The same for the formulas a master shape holds itself, which every instance of it reads on itself.
    fn master_intra_of(&mut self, ev: &mut Evaluator<'_, '_>, shape: &ShapeRef) -> Intra {
        let sheet = SheetRef::Shape(shape.clone());
        if let Some(known) = self.master_intra.get(&sheet) {
            return Rc::clone(known);
        }
        let built: Intra = Rc::new(build_intra(ev, &sheet, true, &mut self.formulas));
        self.master_intra.insert(sheet, Rc::clone(&built));
        built
    }
}

/// The cells of `sheet` whose formula names a cell of the sheet itself, by the cell named. A cell with `Inh` has the formula the chain gives
/// it; with `local_only` only the formulas the sheet holds are looked at (a master shape's, which its instances read on themselves).
fn build_intra(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, local_only: bool, formulas: &mut SectionFormulas) -> HashMap<KeyCell, Vec<CellAddr>> {
    let mut map: HashMap<KeyCell, Vec<CellAddr>> = HashMap::new();
    let Some(data) = ev.res.sheet(sheet) else { return map };
    let mut cells: Vec<(CellAddr, Option<Arc<Formula>>)> = Vec::new();
    each_formula(data, formulas, |addr, formula| match formula {
        Some(formula) => cells.push((addr.clone(), Some(Arc::clone(formula)))),
        None if !local_only => cells.push((addr.clone(), None)),
        None => {}
    });
    for (addr, held) in cells {
        let formula = match held {
            Some(formula) => formula,
            None => {
                let from_chain = ev.res.resolve(sheet, &addr).and_then(|r| match r.formula.cell.map(|c| &c.formula) {
                    Some(FormulaSlot::Local(formula)) => Some(Arc::clone(formula)),
                    _ => None,
                });
                let Some(formula) = from_chain else { continue };
                formula
            }
        };
        if !ev.steps(1) {
            break;
        }
        let Some(facts) = ev.facts_of(&formula) else { continue };
        let reads_path = calls_any(&facts.functions, &PATH_FUNCTIONS);
        for r in facts.links().filter(|r| r.qualifiers.is_empty()) {
            if let Some(named) = address_of(&r.path) {
                if !ev.ctx.hold_bytes(KEY_BYTES) {
                    return map;
                }
                map.entry(place_of(&named)).or_default().push(addr.clone());
                // A path function reads the whole section its first argument names, and the size and rounding of the shape.
                if reads_path {
                    for place in path_reads(&named) {
                        if !ev.ctx.hold_bytes(KEY_BYTES) {
                            return map;
                        }
                        map.entry(place).or_default().push(addr.clone());
                    }
                } else if let Some(place) = written_whole(&named) {
                    // A cell of a section a command writes whole is read by this formula too when the section is written (`Changes::sections`).
                    if !ev.ctx.hold_bytes(KEY_BYTES) {
                        return map;
                    }
                    map.entry(place).or_default().push(addr.clone());
                }
            }
        }
    }
    map
}
