//! **The oracle: the walk of the whole page that every edit made before a change, kept as it was** (only this note, the visibility of a few items and
//! the doc comments that say what `index.rs` now does are changed; and gave it the rule of `affected.rs` for a reference that finds no sheet, which
//! the index keeps the `ID` of, and `changes_between`'s shape-by-shape comparison of a list that repeats an `ID`, which this walk shares; and gave it the rule that a
//! reference which is only the argument of `GETREF` or `GETVAL` is no reader, `facts.links()`, which the index takes too). It lists the readers of other sheets by asking every formula of the page for each
//! edit, which the index kept from one edit to the next replaced. The tests of `affected/tests.rs` hold the index to this walk: the same cells handed
//! out, in the same order, over generated pages and edits, and the same document after a commit.
//!
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
//! **When the walk is by the whole page instead.** `changes_between` cannot compare a page in which two shapes of one list hold one `ID`, and
//! reports [`Changes::whole_page`]: every formula of the page is worked out, which writes more, and does so without a word. The
//! editing commands name the `ID` of a shape they make above every one on the page, so only a file with duplicates is met by it.
//!
//! **What reads what.** A cell reads another when
//!
//! * its formula names it: unqualified (`Width`, `Geometry1.X2`: a cell of the cell's own sheet), or qualified (`Sheet.12!PinX`, `ThePage!User.Row`),
//!   found as the evaluator finds it ([`Evaluator::locate`]), so a name in a formula found on a master is read for the instance
//!   A row is told by its key; where a section's rows may be keyed by name or by place the key is dropped, and a name
//!   that reaches any row of that section reads every row of it: more cells are worked out than needed and none fewer (the references that are the argument of
//!   `GETREF` or `GETVAL` are no reads, as `affected.rs` says: this walk takes [`crate::facts::Facts::links`] too);
//! * its formula calls a function that carries a point or an angle from one frame to another (`PAR`, `LOC`, `LOCTOLOC`, `LOCTOPAR`,
//!   `ANGLETOLOC`, `ANGLETOPAR`), which reads the pose cells (`PinX`, `PinY`, `LocPinX`, `LocPinY`, `Angle`, `FlipX`, `FlipY`) of its own shape and
//!   its groups, and of every shape its references name and their groups. This is how a connector glued by `PAR(PNT(Sheet.4!Connections.X1, ...))`
//!   follows the shape it is glued to;
//! * a `Connect` row says its end is glued to a shape: the end's cells read the pose of that shape, whatever their formulas are;
//! * it inherits a formula that reads the cell: an instance's `Inh` cell, or a cell it does not hold at all and gets from its master, with the
//!   master's references read on the instance.
//!
//! **What is always worked out.** A cell that reads what its text does not name: a reference with a call before it (`LISTSHEETREF()!Width`),
//! or a function that reads the members of a list, a container or a callout, or a string as a formula ([`crate::facts::Facts::dynamic`]), is
//! worked out after every edit that changed anything, and so is a cell with a reference that finds no sheet after an edit that took a shape out
//! (it may have named it). **A section written whole** (`Changes::sections`: a route replaces the rows of a connector's `Geometry` section) wakes the readers of any cell of it,
//! which a reference to a `Geometry` cell is listed under (`some_of`), as `affected.rs` says. **What is not followed.** What `THEMEVAL`, `TEXTWIDTH` and the like read of the sheet's theme cells and text: the
//! editing commands change none of those, and a caller whose edit does sets [`Changes::whole_page`].
//!
//! **What it costs.** The references of a formula are found once for each formula (they are kept under the formula's address, `facts.rs`); the
//! cells of a sheet that read one of its own cells are found when a change first reaches the sheet, once for a sheet and once for a master shape,
//! whatever the number of its instances; and the references to other sheets, the frame functions and the `Connect` rows are listed once for the
//! page. Each step is charged to the budget.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use diacraft_doc::{CellAddr, CellName, Document, Formula, FormulaSlot, Keying, Owner, Rebind, RowKey, SectionKey, ShapeRef, SheetRef, keying};

use crate::changes::{Changes, each_cell, end_cells};
use crate::eval::{Env, Evaluator};
use crate::facts::{PATH_FUNCTIONS, TEXT_MEASURES, TextRead, reach};
use crate::recalc::walk_page;
use crate::refs::address_of;
use crate::rules::KEY_BYTES;

/// Where a cell is in its sheet: the section, the row, and the cell's name. The row is left out for a section whose rows may be keyed by name or
/// by place (`Keying::Either`), where a reference and a row might name one row two ways: a name that reaches a row of such a section reads every
/// row of it, so more cells are worked out than need be and none fewer. In a section keyed by name (`User`, `Prop`) or by place (`Geometry`) a
/// row is told by its key, which a reference and the sheet spell alike.
type KeyCell = (Option<SectionKey>, Option<RowKey>, CellName);
/// A cell as something is read through: its sheet and its place.
type Key = (SheetRef, KeyCell);
/// A cell that reads: its sheet and its exact address.
pub(crate) type Reader = (SheetRef, CellAddr);

/// The cells that set a shape's frame: what a function that carries a point between frames reads.
const POSE: [&str; 7] = ["PinX", "PinY", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY"];

/// The name the seven pose cells of a shape are read under together: the readers of a frame are listed once for a shape, under this, and not
/// seven times. No cell has it.
const POSE_GROUP: &str = "\u{0}pose";

/// The name a wildcard is kept under in the place of a cell: "any cell of this section". No cell has it.
const ANY_CELL: &str = "\u{0}any";

/// The place that stands for every cell of a section (`Geometry1.*`, `Char.*`): a function that reads a whole section is listed under it, and a
/// change to any cell of the section finds it.
fn any_of(section: &SectionKey) -> KeyCell {
    (Some(section.clone()), None, CellName::new(ANY_CELL))
}

/// The name a reader of one cell of a section is also kept under: "some cell of this section is read here". No cell has it (the rule of `affected.rs` for a section written whole).
const SOME_CELL: &str = "\u{0}some";

/// The place a reader of one cell of a section is also listed under, for a change to the section as a whole (`Changes::sections`).
fn some_of(section: &SectionKey) -> KeyCell {
    (Some(section.clone()), None, CellName::new(SOME_CELL))
}

/// The place a reader of the cell `named` is also listed under, when the cell is one of a `Geometry` section, which a route replaces whole (`affected.rs`).
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
fn pose_keys(sheet: &SheetRef) -> Vec<Key> {
    chain(sheet).into_iter().map(|s| (s, (None, None, CellName::new(POSE_GROUP)))).collect()
}

/// Cells of a sheet with the formulas they hold.
type Formulas = Vec<(CellAddr, Arc<Formula>)>;

/// The cells of one sheet that read a cell of the same sheet, by the cell they read.
type Intra = Rc<HashMap<KeyCell, Vec<CellAddr>>>;

#[derive(Default)]
pub(super) struct Graph {
    /// The readers of a cell through a qualified reference, a frame function or a `Connect` row, by the cell they read.
    pub(super) cross: HashMap<Key, Vec<Reader>>,
    /// The readers of cells their text does not name.
    pub(super) dynamic: Vec<Reader>,
    /// The readers with a qualified reference that finds no sheet.
    pub(super) dangling: Vec<(Reader, Option<u32>)>,
    /// The readers of a cell of their own sheet, for each sheet a change has reached.
    intra: HashMap<SheetRef, Intra>,
    /// The same for the formulas a master shape holds: one for all its instances.
    master_intra: HashMap<SheetRef, Intra>,
    /// The formulas of a master shape that read other sheets, by the master shape.
    master_cross: HashMap<SheetRef, Rc<Formulas>>,
}

/// The cells to work out for an edit, found as they are worked out (of the M3 review). The edit's own cells ([`Changes`]) and the cells
/// that are always worked out are there from the start; **the readers of a cell are noted when the cell changed**: for a cell the edit wrote, or
/// that is new, at once; for any other, when working it out ([`crate::commit_changed`]) planned a write ([`Affected::changed`]). A reader that comes
/// out as it was ends the chain, and so does a cell this evaluator cannot work out (a function it does not know): what reads those keeps the
/// value the file saved, whatever this evaluator would give for it. A connector end glued by a function that `calc` cannot work out, and
/// so left as it was, does not pass a move on to the width and the pin that read it, which would come out a rounding away from the file's own.
pub(crate) struct Affected {
    graph: Graph,
    /// Every cell noted, once.
    seen: HashSet<Reader>,
    /// The cells noted and not yet handed out, in the order they were found.
    todo: VecDeque<Reader>,
    /// The cells whose readers have been noted.
    followed: HashSet<Key>,
}

impl Affected {
    /// The cells to start from for `changes` on page `page`: the cells changed, the cells of a shape that is new, the cells that read a shape
    /// that was taken out, and the cells that are always worked out.
    pub(crate) fn new(ev: &mut Evaluator<'_, '_>, doc: &Document, page: u32, changes: &Changes) -> Affected {
        let mut graph = Graph::default();
        list_cross_readers(ev, doc, page, &mut graph);
        let mut work = Affected { graph, seen: HashSet::new(), todo: VecDeque::new(), followed: HashSet::new() };
        // What the edit changed is changed whatever working it out gives: its readers are noted below.
        let mut changed: Vec<Key> = Vec::with_capacity(changes.cells.len());
        for (sheet, addr) in &changes.cells {
            work.add(ev, (sheet.clone(), addr.clone()));
            changed.push(key_of(sheet, addr));
        }
        // A row or a section that appeared, went or took another type changed what a function that reads the whole section reads, though no cell
        // of it was written: it counts as a change to "any cell" of the section.
        for (sheet, section, row) in &changes.rows {
            let any = CellAddr { section: Some(section.clone()), row: row.clone(), cell: CellName::new(ANY_CELL) };
            changed.push(key_of(sheet, &any));
        }
        // A section written whole changed every cell it holds (`Changes::sections`, the rule of `affected.rs`).
        for (sheet, section) in &changes.sections {
            changed.push((sheet.clone(), some_of(section)));
        }
        for sheet in &changes.fresh {
            work.add_fresh(ev, sheet, &mut changed);
        }
        let (dangling, dynamic) = (std::mem::take(&mut work.graph.dangling), std::mem::take(&mut work.graph.dynamic));
        // A reference that finds no sheet, by the rule of `affected.rs`: a plain `Sheet.<id>` only when that `ID` was taken out.
        if !changes.removed.is_empty() {
            let removed: HashSet<u32> = changes.removed.iter().copied().collect();
            dangling.into_iter().filter(|(_, named)| named.is_none_or(|id| removed.contains(&id))).for_each(|(r, _)| work.add(ev, r));
        }
        if !changes.cells.is_empty()
            || !changes.rows.is_empty()
            || !changes.sections.is_empty()
            || !changes.fresh.is_empty()
            || !changes.removed.is_empty()
        {
            dynamic.into_iter().for_each(|r| work.add(ev, r));
        }
        for key in changed {
            work.follow(ev, key);
        }
        work
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
        for reader in self.graph.readers_of(ev, &key) {
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
}

/// Lists, for the page, the readers of other sheets: formulas with a qualified reference or a frame function (those a shape holds, and those
/// it inherits), the cells that read what their text does not name, and the end cells of each glued connector.
pub(super) fn list_cross_readers(ev: &mut Evaluator<'_, '_>, doc: &Document, page: u32, graph: &mut Graph) {
    walk_page(doc, page, &mut |sheet, data| {
        let own = Env::new(sheet.clone(), None);
        each_cell(data, |addr, cell| {
            if let FormulaSlot::Local(formula) = &cell.formula {
                classify(ev, graph, sheet, &addr, formula, &own);
            }
        });
        let SheetRef::Shape(at) = sheet else { return };
        let Ok(Some(step)) = ev.res.master_step(at) else { return };
        let inherited = graph.master_cross_of(ev, &step.shape);
        if inherited.is_empty() {
            return;
        }
        let env = Env::new(sheet.clone(), Some(Rebind { instance: step.instance, master: step.master }));
        for (addr, formula) in inherited.iter() {
            classify(ev, graph, sheet, addr, formula, &env);
        }
    });
    let Some(found) = doc.page(page) else { return };
    for connect in found.connects.iter() {
        let Some(from_cell) = connect.from_cell.as_deref() else { continue };
        if !ev.steps(1) {
            return;
        }
        let (Some(from), Some(to)) = (ev.shape_by_id(Owner::Page(page), connect.from_sheet), ev.shape_by_id(Owner::Page(page), connect.to_sheet))
        else {
            continue;
        };
        let to_keys = pose_keys(&SheetRef::Shape(to));
        for name in end_cells(from_cell) {
            let reader = (SheetRef::Shape(from.clone()), CellAddr::direct(name));
            for key in &to_keys {
                if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                    return;
                }
                graph.cross.entry(key.clone()).or_default().push(reader.clone());
            }
        }
    }
}

/// Whether a formula reads anything but unqualified cells of its own sheet ([`crate::facts::Reach`]); each node is a step.
fn reaches_beyond_own_sheet(ev: &mut Evaluator<'_, '_>, formula: &Formula) -> bool {
    let Ok(expr) = &formula.ast else { return false };
    let (found, nodes) = reach(expr);
    ev.steps(nodes) && found.beyond_own_sheet()
}

/// Records what the formula of the cell `addr` of `sheet` reads beyond the sheet's own cells, if anything.
fn classify(ev: &mut Evaluator<'_, '_>, graph: &mut Graph, sheet: &SheetRef, addr: &CellAddr, formula: &Arc<Formula>, env: &Env) {
    // Most formulas name nothing but cells of their own sheet: they are told by a walk that allocates nothing, and their references are not listed.
    if !reaches_beyond_own_sheet(ev, formula) {
        return;
    }
    let Some(facts) = ev.facts_of(formula) else { return };
    let reader: Reader = (sheet.clone(), addr.clone());
    if facts.dynamic {
        graph.dynamic.push(reader.clone());
    }
    // The shapes whose frames a function carries between: each one a reference names, the groups the cell's own shape is in, and the shape itself
    // where the formula reads its pose too (`Facts::frame_own`).
    let mut frames: Vec<SheetRef> = Vec::new();
    if facts.frame {
        frames.extend(if facts.frame_own { Some(sheet.clone()) } else { parent_of(sheet) });
    }
    let reads_path = calls_any(&facts.functions, &PATH_FUNCTIONS);
    for r in facts.links().filter(|r| !r.qualifiers.is_empty()) {
        if !ev.steps(1) || !ev.ctx.hold_bytes(2 * KEY_BYTES) {
            return;
        }
        match ev.locate(r, env) {
            Ok((target, at)) => {
                graph.cross.entry(key_of(&target, &at)).or_default().push(reader.clone());
                // A path function reads the whole section its first argument names, on the sheet the reference names.
                if reads_path {
                    for place in path_reads(&at) {
                        if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                            return;
                        }
                        graph.cross.entry((target.clone(), place)).or_default().push(reader.clone());
                    }
                } else if let Some(place) = written_whole(&at) {
                    if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                        return;
                    }
                    graph.cross.entry((target.clone(), place)).or_default().push(reader.clone());
                }
                if facts.frame {
                    frames.push(target);
                }
            }
            Err(_) => graph.dangling.push((reader.clone(), super::plain_sheet_id(r, env))),
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
                if !ev.steps(1) || !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                    return;
                }
                graph.cross.entry((holder.clone(), place)).or_default().push(reader.clone());
            }
            if let Some(page) = scale_sheet {
                for name in ["PageScale", "DrawingScale"] {
                    if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                        return;
                    }
                    graph.cross.entry((page.clone(), direct_place(name))).or_default().push(reader.clone());
                }
            }
        }
    }
    for frame in &frames {
        for key in pose_keys(frame) {
            if !ev.ctx.hold_bytes(2 * KEY_BYTES) {
                return;
            }
            graph.cross.entry(key).or_default().push(reader.clone());
        }
    }
}

impl Graph {
    /// The cells that read the cell `key`: through other sheets, in its own sheet, and in an instance of its sheet's master shape.
    fn readers_of(&mut self, ev: &mut Evaluator<'_, '_>, key: &Key) -> Vec<Reader> {
        let mut out: Vec<Reader> = self.cross.get(key).cloned().unwrap_or_default();
        let (sheet, place) = key;
        // A function that reads a whole section is listed under the section's wildcard, and reads every cell of it.
        let wildcard = place.0.as_ref().map(any_of);
        if let Some(any) = &wildcard {
            out.extend(self.cross.get(&(sheet.clone(), any.clone())).into_iter().flatten().cloned());
        }
        if place.0.is_none() && place.1.is_none() && POSE.contains(&place.2.as_str()) {
            let group = (sheet.clone(), (None, None, CellName::new(POSE_GROUP)));
            out.extend(self.cross.get(&group).into_iter().flatten().cloned());
        }
        if matches!(sheet, SheetRef::Shape(_) | SheetRef::Page(_)) {
            let own = self.intra_of(ev, sheet);
            out.extend(own.get(place).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            if let Some(any) = &wildcard {
                out.extend(own.get(any).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            }
        }
        if let SheetRef::Shape(at) = sheet
            && matches!(at.owner, Owner::Page(_))
            && let Ok(Some(step)) = ev.res.master_step(at)
        {
            let inherited = self.master_intra_of(ev, &step.shape);
            out.extend(inherited.get(place).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            if let Some(any) = &wildcard {
                out.extend(inherited.get(any).into_iter().flatten().map(|addr| (sheet.clone(), addr.clone())));
            }
        }
        out
    }

    /// The cells of `sheet` that read a cell of `sheet`, by the cell read: found when first asked.
    fn intra_of(&mut self, ev: &mut Evaluator<'_, '_>, sheet: &SheetRef) -> Intra {
        if let Some(known) = self.intra.get(sheet) {
            return Rc::clone(known);
        }
        let built: Intra = Rc::new(build_intra(ev, sheet, false));
        self.intra.insert(sheet.clone(), Rc::clone(&built));
        built
    }

    /// The same for the formulas a master shape holds itself, which every instance of it reads on itself.
    fn master_intra_of(&mut self, ev: &mut Evaluator<'_, '_>, shape: &ShapeRef) -> Intra {
        let sheet = SheetRef::Shape(shape.clone());
        if let Some(known) = self.master_intra.get(&sheet) {
            return Rc::clone(known);
        }
        let built: Intra = Rc::new(build_intra(ev, &sheet, true));
        self.master_intra.insert(sheet, Rc::clone(&built));
        built
    }

    /// The formulas a master shape holds that read other sheets (or what their text does not name): found once for the shape.
    fn master_cross_of(&mut self, ev: &mut Evaluator<'_, '_>, shape: &ShapeRef) -> Rc<Formulas> {
        let sheet = SheetRef::Shape(shape.clone());
        if let Some(known) = self.master_cross.get(&sheet) {
            return Rc::clone(known);
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
        let list = Rc::new(list);
        self.master_cross.insert(sheet, Rc::clone(&list));
        list
    }
}

/// The cells of `sheet` whose formula names a cell of the sheet itself, by the cell named. A cell with `Inh` has the formula the chain gives
/// it; with `local_only` only the formulas the sheet holds are looked at (a master shape's, which its instances read on themselves).
fn build_intra(ev: &mut Evaluator<'_, '_>, sheet: &SheetRef, local_only: bool) -> HashMap<KeyCell, Vec<CellAddr>> {
    let mut map: HashMap<KeyCell, Vec<CellAddr>> = HashMap::new();
    let Some(data) = ev.res.sheet(sheet) else { return map };
    let mut cells: Vec<(CellAddr, Option<Arc<Formula>>)> = Vec::new();
    each_cell(data, |addr, cell| match &cell.formula {
        FormulaSlot::Local(formula) => cells.push((addr, Some(Arc::clone(formula)))),
        FormulaSlot::Inherit if !local_only => cells.push((addr, None)),
        _ => {}
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
