//! The core of the format commands: the fill, line and text cells a person's formatting writes, and the resolved look the window's
//! controls show.
//!
//! * [`format_shapes`]: the Fill and Line cells (`shape.format`).
//! * [`format_text`]: the Character and Paragraph rows and the text block's `VerticalAlign` (`text.format`); a range of characters splits runs
//!   with their formatting kept (`runs.rs`).
//! * [`look`]: what a shape's cells resolve to, for display (`shape.formatting`).
//!
//! **What a formatting command writes** is a local cell on the shape (an instance holds what differs), the way a file holds it:
//! a colour is `F="RGB(r,g,b)"` with `V="#rrggbb"` in lower case; a number is a value with no
//! formula. A cell that was a formula (`THEMEVAL()`, `THEMEGUARD(...)`) becomes the value, as any write from the drawing window replaces the
//! formula of a cell that is not guarded, and the theme is not changed. A `GUARD` cell is an error, and nothing
//! changes. A cell that already holds what would be written is not written, so a call that changes nothing is no undo step.
//!
//! **Recalculation** follows: the cells written, and what reads them, are worked out again (`calc::commit_changed` with exactly the
//! cells written), not the page. Nothing here moves a shape, so no connector is routed again.
//!
//! **Groups** (ours): a group is formatted through its members. Every shape without members inside the shapes named, to any
//! depth, is formatted, and the cells of the group itself are left alone: a group draws nothing of its own. **A group one of whose members to be formatted is
//! on a locked layer is refused in words** (a shape on a locked layer is not edited, and the specifications say nothing of a group whose
//! member is on one), each member judged by its own list.

mod effects;
mod face;
mod look;
mod quick;
mod runs;
mod shape_cells;
#[cfg(test)]
mod tests;
mod text;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, Changes};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, CellName, Document, FormulaSlot, Lookup, MAX_SHAPE_DEPTH, Resolver, RowKey, SectionKey, ShapeRef, SheetRef, Value,
};

use crate::calc_index::commit_changed;
use crate::draw::{Problem, require_shape, shape_mut, write_cells};

pub use effects::{
    GradientFormat, GradientKind, GradientLook, GradientTarget, MAX_GRADIENT_STOPS, SHADOW_MAX_POINTS, SHADOW_PRESETS, SHADOW_SIZE_RANGE,
    ShadowFormat, ShadowLook, StopFormat, StopLook, format_gradient, format_shadow, shadow_preset,
};
pub use look::{Align, Case, Direction, FxLook, Indent, Mixed, RangeLook, ShapeLook, VAlign, look_of, look_of_in, look_of_range, look_of_range_in};
/// The resolved reading of cells the window's controls use, for the `.odg` save to flatten a new shape by (`odgfile/flatten.rs`).
pub(crate) use look::{Reader as LookReader, Val as LookVal, eval_ctx as look_eval_ctx};
pub use quick::{MAX_QUICK_SHAPES, QuickLook, QuickStyle, QuickTile, format_quick_style, quick_tiles};
pub use shape_cells::{Paint, ShapeFormat, format_shapes, parse_paint};
pub use text::{MAX_BULLET, MAX_SIZE_POINTS, TextFormat, TextPaint, format_text};
/// For the Kani harnesses of the crate (`crate::kani_proofs`).
#[cfg(kani)]
pub(crate) use {
    effects::radians_within_turn,
    look::SMALL_CAPS_BIT,
    shape_cells::share,
    text::{INDENT_STEP, MAX_INDENT, indent_to},
};

/// Most shapes one command formats, groups opened: a group of ten thousand members is one command, and a hundred thousand is not.
pub const MAX_TARGETS: usize = 20_000;

/// Most shapes a colour of the theme is put on in one command: **each shape is made to follow the page's theme** (`follow_page_theme_refs`, one pass over the shapes named), and the cells
/// a shape is given are worked out again, so the count is bounded before any pass is made. A selection of more is refused in words (a plain colour is held to [`MAX_TARGETS`] alone).
pub const MAX_PICK_SHAPES: usize = crate::theme::MAX_FOLLOWED_SHAPES;

/// Most face names the font table holds when a command adds one: real files hold up to a few hundred, so this is generous.
pub const MAX_FACE_NAMES: usize = 4_096;

/// A colour as the cells of a file hold it: the formula and the lower-case hex value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` or `#RRGGBB`, and nothing else: the colour a person or a program names.
    pub fn parse(text: &str) -> Option<Rgb> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| digits.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    /// `RGB(60,120,216)`.
    pub fn formula(self) -> String {
        format!("RGB({},{},{})", self.0, self.1, self.2)
    }

    /// `#3c78d8`.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// What a command means to put in a cell.
#[derive(Clone, Debug)]
pub(crate) enum New {
    /// A value of its own and no formula.
    Value(String),
    /// A colour: its formula and its value.
    Colour(Rgb),
    /// The theme's answer: the formula `THEMEVAL()` with the marker `Themed` as its value, as the `Theme` style and the shapes a Quick Style was put on hold it.
    Themed,
    /// A colour picked from the page's theme (`theme/palette.rs`): its formula (`THEMEGUARD(THEMEVAL("AccentColor2"))`) and the colour it works out to now, which the
    /// cell holds until the recalculation that follows the write works it out on the shape.
    Theme { formula: String, value: String },
}

impl New {
    pub(crate) fn number(n: f64) -> New {
        New::Value(Value::from_number(n).map_or_else(|| "0".to_owned(), |v| v.text().to_owned()))
    }

    pub(crate) fn whole(n: u32) -> New {
        New::Value(n.to_string())
    }

    /// `old` (the cell as the sheet holds it, or nothing) with this written into it: the unit, the other attributes and the text it had are kept.
    pub(crate) fn written_over(&self, old: Option<&Cell>) -> Cell {
        let mut cell = old.cloned().unwrap_or_default();
        match self {
            New::Value(text) => {
                cell.formula = FormulaSlot::Absent;
                cell.value = Some(Value::from_text(text));
            }
            New::Colour(rgb) => {
                let fresh = Cell::with_formula(&rgb.formula(), Some(Value::from_text(&rgb.hex())));
                cell.formula = fresh.formula;
                cell.value = fresh.value;
            }
            New::Themed => {
                let fresh = Cell::with_formula("THEMEVAL()", Some(Value::from_text("Themed")));
                cell.formula = fresh.formula;
                cell.value = fresh.value;
            }
            New::Theme { formula, value } => {
                let fresh = Cell::with_formula(formula, Some(Value::from_text(value)));
                cell.formula = fresh.formula;
                cell.value = fresh.value;
            }
        }
        cell.error = None;
        cell
    }
}

/// A row of a section of a shape, to be made or made again as these cells and no others.
pub(crate) struct RowSet {
    sheet: SheetRef,
    section: &'static str,
    ix: u32,
    cells: BTreeMap<CellName, Cell>,
    state: RowState,
}

/// How a planned row is put in a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowState {
    /// The row of a text (`Character`, `Paragraph`): a row or section that is deleted here is an error, a write into it would be lost.
    Text,
    /// A row that is made, or made again, **and brought back** when the shape has it deleted (`Del`): the rows of a gradient's stops, where a row a shape deleted is
    /// asked for again when the person adds a stop.
    Revived,
    /// A row the shape deletes (`Del`), with no cell: how a shape leaves out a row its style holds.
    Deleted,
}

/// A row of a section of a shape, to be taken out.
pub(crate) struct RowGone {
    sheet: SheetRef,
    section: &'static str,
    ix: u32,
}

/// The writes a command plans, against the document as it is: cells, whole rows, and rows taken out.
#[derive(Default)]
pub(crate) struct Edits {
    pub writes: Vec<(SheetRef, CellAddr, CellEdit)>,
    /// Where the writes for the shape being planned begin, and where in `writes` each cell planned since then is: a cell planned twice for one
    /// shape is found by its address, in one step, however many cells the shape has planned (a text of thousands of runs plans thousands).
    scope: usize,
    index: HashMap<(SheetRef, CellAddr), usize>,
    rows: Vec<RowSet>,
    gone: Vec<RowGone>,
}

impl Edits {
    /// Starts the writes for another shape.
    pub(crate) fn begin(&mut self) {
        self.scope = self.writes.len();
        self.index.clear();
    }

    /// Takes the writes, rows and rows taken out of another set of edits, which are for another shape: what is planned next is planned afresh.
    pub(crate) fn append(&mut self, other: Edits) {
        self.writes.extend(other.writes);
        self.rows.extend(other.rows);
        self.gone.extend(other.gone);
        self.begin();
    }

    /// Plans `new` for the cell at `addr` of `sheet`. Nothing is planned when the cell holds that already, and a cell planned before for this
    /// shape is planned again instead (the later wish wins, and is dropped if it is what the cell holds). → whether a write is planned for the
    /// cell now.
    pub(crate) fn put(&mut self, doc: &Document, sheet: &SheetRef, addr: CellAddr, new: &New) -> bool {
        let old = local_cell(doc, sheet, &addr);
        let cell = new.written_over(old);
        let same = old == Some(&cell);
        let key = (sheet.clone(), addr);
        let planned = self.index.get(&key).copied();
        match (planned, same) {
            (None, true) => false,
            (None, false) => {
                self.index.insert(key.clone(), self.writes.len());
                self.writes.push((key.0, key.1, CellEdit::Cell(cell)));
                true
            }
            (Some(at), true) => {
                // The last write takes its place, so the others keep theirs; its entry follows it.
                self.index.remove(&key);
                self.writes.swap_remove(at);
                if let Some((s, a, _)) = self.writes.get(at) {
                    self.index.insert((s.clone(), a.clone()), at);
                }
                false
            }
            (Some(at), false) => {
                if let Some(slot) = self.writes.get_mut(at) {
                    slot.2 = CellEdit::Cell(cell);
                }
                true
            }
        }
    }

    /// Plans the row `ix` of a section of a shape to hold exactly `cells`: what the row holds now is replaced, and a cell it holds that `cells` do
    /// not is taken out.
    pub(crate) fn set_row(&mut self, sheet: &SheetRef, section: &'static str, ix: u32, cells: BTreeMap<CellName, Cell>) {
        #[cfg(test)]
        counts::ROWS_SET.with(|c| c.set(c.get() + 1));
        self.rows.push(RowSet { sheet: sheet.clone(), section, ix, cells, state: RowState::Text });
    }

    /// Plans the row `ix` of a section of a shape to hold exactly `cells`, **brought back** if the shape had it deleted.
    pub(crate) fn set_revived_row(&mut self, sheet: &SheetRef, section: &'static str, ix: u32, cells: BTreeMap<CellName, Cell>) {
        self.rows.push(RowSet { sheet: sheet.clone(), section, ix, cells, state: RowState::Revived });
    }

    /// Plans the row `ix` of a section of a shape to be **deleted here** (`Del`): its cells, and the row's of a style or a master, are not read.
    pub(crate) fn set_deleted_row(&mut self, sheet: &SheetRef, section: &'static str, ix: u32) {
        self.rows.push(RowSet { sheet: sheet.clone(), section, ix, cells: BTreeMap::new(), state: RowState::Deleted });
    }

    /// Plans the row `ix` of a section of a shape to be taken out.
    pub(crate) fn remove_row(&mut self, sheet: &SheetRef, section: &'static str, ix: u32) {
        self.gone.push(RowGone { sheet: sheet.clone(), section, ix });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.rows.is_empty() && self.gone.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.writes.len()
    }

    /// What is planned: the cells, the rows made and the rows taken out.
    pub(crate) fn planned(&self) -> usize {
        self.writes.len().saturating_add(self.rows.len()).saturating_add(self.gone.len())
    }

    /// Makes the writes (all or nothing, a `GUARD` cell an error) and works out what they changed and what reads it.
    pub(crate) fn apply(self, doc: &mut Document, page: u32) -> Result<(), Problem> {
        let changed = self.write(doc, page)?;
        recalculate(doc, page, changed)
    }

    /// [`Edits::apply`] without the recalculation: for a copy of the document that a preview reads the cells of, where a cell that holds the theme's marker is
    /// asked of the theme as it is read, and nothing is kept.
    pub(crate) fn apply_to_preview(self, doc: &mut Document, page: u32) -> Result<(), Problem> {
        self.write(doc, page).map(|_| ())
    }

    /// The writes, the rows made and the rows taken out. → the cells that changed.
    fn write(self, doc: &mut Document, page: u32) -> Result<Vec<(SheetRef, CellAddr)>, Problem> {
        let mut changed: Vec<(SheetRef, CellAddr)> = self.writes.iter().map(|(sheet, addr, _)| (sheet.clone(), addr.clone())).collect();
        write_cells(doc, self.writes)?;
        for set in self.rows {
            changed.extend(set.cells.keys().map(|name| (set.sheet.clone(), CellAddr::in_row(set.section, None, RowKey::Ix(set.ix), name.as_str()))));
            replace_row(doc, page, set)?;
        }
        // The rows to take out, by section: one pass over a section takes all of its rows out (a pass for each row would copy the section's rows
        // again for each, and a text may leave thousands behind).
        let mut gone: HashMap<(SheetRef, &'static str), HashSet<u32>> = HashMap::new();
        for row in self.gone {
            gone.entry((row.sheet, row.section)).or_default().insert(row.ix);
        }
        for ((sheet, section), rows) in gone {
            remove_rows(doc, page, &sheet, section, &rows)?;
        }
        Ok(changed)
    }
}

/// The row of a section a shape holds itself, if it holds one that is in force: not deleted, in a section that is not deleted.
fn local_row<'a>(doc: &'a Document, sheet: &SheetRef, section: &str, ix: u32) -> Result<Option<&'a Arc<diacraft_doc::Row>>, Problem> {
    let Some(section) = doc.sheet_of(sheet).and_then(|s| s.section(&SectionKey::new(section, None))) else { return Ok(None) };
    if section.del {
        return Err(Problem::failed("the rows of the text are deleted here (Del), so a write into them would be lost"));
    }
    match section.row(&RowKey::Ix(ix)) {
        Some(row) if row.del => Err(Problem::failed("a row of the text is deleted here (Del), so a write into it would be lost")),
        found => Ok(found),
    }
}

/// Whether the shape holds the row `ix` of a section itself, with cells in it.
pub(crate) fn holds_row(doc: &Document, sheet: &SheetRef, section: &str, ix: u32) -> bool {
    local_row(doc, sheet, section, ix).ok().flatten().is_some_and(|row| !row.cells.is_empty())
}

/// Whether the shape holds the row `ix` of a section itself, in force, with exactly `cells`.
pub(crate) fn holds_exactly(doc: &Document, sheet: &SheetRef, section: &str, ix: u32, cells: &BTreeMap<CellName, Cell>) -> bool {
    local_row(doc, sheet, section, ix).ok().flatten().is_some_and(|row| row.cells == *cells)
}

/// Makes a row of a shape hold the cells of `set`. A `GUARD` cell the row holds must be one the new cells hold the same (it is moved, not changed).
fn replace_row(doc: &mut Document, page: u32, set: RowSet) -> Result<(), Problem> {
    let SheetRef::Shape(at) = &set.sheet else { return Err(Problem::failed("only a shape holds the rows of a text")) };
    let held = match set.state {
        RowState::Text => local_row(doc, &set.sheet, set.section, set.ix)?,
        // A row that is deleted here is read as the row it is, to be made again or left deleted.
        RowState::Revived | RowState::Deleted => {
            doc.sheet_of(&set.sheet).and_then(|s| s.section(&SectionKey::new(set.section, None))).and_then(|section| section.row(&RowKey::Ix(set.ix)))
        }
    };
    if let Some(row) = held
        && row.cells.iter().any(|(name, cell)| cell.is_guarded() && set.cells.get(name) != Some(cell))
    {
        return Err(Problem::failed(if set.state == RowState::Text {
            "a cell of the text is protected by GUARD and cannot be changed by formatting"
        } else {
            "a cell of a row of the shape is protected by GUARD and cannot be changed by formatting"
        }));
    }
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let sheet = Arc::make_mut(&mut shape.sheet);
    let key = SectionKey::new(set.section, None);
    if set.state == RowState::Revived {
        // The section brought back too, if the shape had it deleted: a row of a deleted section would still read as gone.
        sheet.section_mut(&key)?.del = false;
    }
    let row = sheet.row_mut(&key, &RowKey::Ix(set.ix))?;
    row.cells = set.cells;
    row.del = set.state == RowState::Deleted;
    Ok(())
}

/// Takes the rows `ixs` out of a shape's section, the ones it holds, in **one pass** over the section: the rows are looked at once, for a deleted
/// section or row and for a `GUARD` cell in a row that is to go (a refusal, before any row is taken out), and then dropped by one `retain`, which
/// also drops the lookup table of the section's rows, so a `retain` for each row would build the table again for each (`k` rows to take out of `n` would
/// cost `k` times `n`).
fn remove_rows(doc: &mut Document, page: u32, sheet: &SheetRef, section: &'static str, ixs: &HashSet<u32>) -> Result<(), Problem> {
    let SheetRef::Shape(at) = sheet else { return Ok(()) };
    let key = SectionKey::new(section, None);
    let goes = |row: &diacraft_doc::Row| matches!(row.key(section), Some(RowKey::Ix(ix)) if ixs.contains(&ix));
    let Some(held) = doc.sheet_of(sheet).and_then(|s| s.section(&key)) else { return Ok(()) };
    if held.del {
        return Err(Problem::failed("the rows of the text are deleted here (Del), so a write into them would be lost"));
    }
    let mut any = false;
    for row in held.rows.iter().filter(|row| goes(row)) {
        any = true;
        if row.del {
            return Err(Problem::failed("a row of the text is deleted here (Del), so a write into it would be lost"));
        }
        if row.cells.values().any(Cell::is_guarded) {
            return Err(Problem::failed("a cell of the text is protected by GUARD and cannot be changed by formatting"));
        }
    }
    if !any {
        return Ok(());
    }
    #[cfg(test)]
    counts::SECTION_PASSES.with(|c| c.set(c.get() + 1));
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the shape went away"))?;
    let section_mut = Arc::make_mut(&mut shape.sheet).section_mut(&key)?;
    section_mut.rows.retain(|row| !goes(row));
    Ok(())
}

/// The cell a sheet holds itself at `addr`, not what it inherits.
pub(crate) fn local_cell<'a>(doc: &'a Document, sheet: &SheetRef, addr: &CellAddr) -> Option<&'a Cell> {
    match doc.sheet_of(sheet)?.lookup(addr) {
        Lookup::Found(cell) => Some(cell),
        _ => None,
    }
}

/// Works out the cells written and every cell that reads one.
fn recalculate(doc: &mut Document, page: u32, cells: Vec<(SheetRef, CellAddr)>) -> Result<(), Problem> {
    let changes = Changes { cells, ..Changes::default() };
    let report = commit_changed(doc, &Budget::default(), crate::measure::eval_options(), page, &changes);
    if report.budget_exhausted || report.memory_exhausted {
        return Err(Problem::failed("the page is too large to recalculate"));
    }
    Ok(())
}

/// Most shapes one command may look at to find the shapes it formats, a group's own node counted as well as its members: shapes named twice, or
/// one inside a group that is named too, are looked at again, so the count is of what is looked at and not of what is kept.
pub const MAX_VISITED: usize = 4 * MAX_TARGETS;

/// The shapes a command formats for the shapes `ids` on a page: each without members, found inside the shapes named to any depth, once
/// In the order the shapes are named. A shape that is not on the page is refused. **The shapes named are found in one walk of the page** that stops at the last of
/// them (`draw::find_named`), where a search for each by `Document::find_shape` walked the page from the top for each: 4,096 names on a page of 30,000 shapes were 58 million visits
/// An `ID` named more than once is walked once;
/// a shape named and also inside a named group is looked at twice, and every shape looked at is charged to one budget, [`MAX_VISITED`], for
/// the whole call, so that a list of names cannot make the walk cost the names times the size of what they hold.
pub(crate) fn targets(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<ShapeRef>, Problem> {
    let mut named: HashSet<u32> = HashSet::with_capacity(ids.len());
    let mut seen: HashSet<Vec<u32>> = HashSet::new();
    let mut out: Vec<ShapeRef> = Vec::new();
    let mut visited = 0_usize;
    // A member of a group named that is on a locked layer is not formatted through the group: each judged by its own list.
    let mut locks = crate::draw::layers::Locks::default();
    // Where each shape named is, in one walk of the page (none named: no walk, and the page is not looked at).
    let places = crate::draw::find_named(doc, page, &ids.iter().copied().collect::<HashSet<u32>>())?;
    for id in ids {
        // Before the walk, so that a name repeated costs nothing; a shape that is not on the page is refused the first time it is named.
        if !named.insert(*id) {
            continue;
        }
        let top = places.get(id).cloned().ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
        let depth = top.path.len();
        let mut stack = vec![top];
        visited = visited.saturating_add(1);
        while let Some(at) = stack.pop() {
            let Some(shape) = doc.shape(&at) else { continue };
            if shape.children.is_empty() || at.path.len() >= MAX_SHAPE_DEPTH {
                if at.path.len() > depth
                    && let Some(layer) = locks.of(doc, page, shape)?
                {
                    return Err(Problem::bad(crate::draw::layers::locked_member_words(*id, shape.id, &layer, "formatted")));
                }
                if seen.insert(at.path.clone()) {
                    if out.len() >= MAX_TARGETS {
                        return Err(Problem::failed(format!("the shapes named hold more than {MAX_TARGETS} shapes to format")));
                    }
                    out.push(at);
                }
                continue;
            }
            // Every member is looked at: charged before any is put down, so that a group of a million members costs one refusal.
            visited = visited.saturating_add(shape.children.len());
            if visited > MAX_VISITED {
                return Err(Problem::failed(format!("the shapes named hold more than {MAX_VISITED} shapes to look through")));
            }
            // Members in order: the stack gives them back last first, so they are pushed last first.
            for member in (0..shape.children.len()).rev() {
                if let Ok(member) = u32::try_from(member) {
                    stack.push(at.child(member));
                }
            }
        }
    }
    Ok(out)
}

/// The first shape with no members inside the shape `id` of a page (the shape itself when it has none), found by going down through the first
/// member at each level: what `shape.formatting` answers for a group. It costs the depth of the group, whatever its size.
pub(crate) fn first_leaf(doc: &Document, page: u32, id: u32) -> Result<ShapeRef, Problem> {
    Ok(leaf_below(doc, require_shape(doc, page, id)?))
}

/// [`first_leaf`] of the shape at `at`, for a caller that has found it (`look_of_in`, which finds the shapes it names in one walk of the page).
pub(crate) fn leaf_below(doc: &Document, mut at: ShapeRef) -> ShapeRef {
    while at.path.len() < MAX_SHAPE_DEPTH && doc.shape(&at).is_some_and(|shape| !shape.children.is_empty()) {
        at = at.child(0);
    }
    at
}

/// The number a cell resolves to by inheritance: its saved value, if that reads as a number. `None` for a cell found nowhere, a theme marker, or
/// text.
pub(crate) fn resolved_number(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> Option<f64> {
    res.resolve(sheet, addr)?.value.cell?.value.as_ref()?.number()
}

/// Whether a cell resolves to a value at all (not found nowhere).
pub(crate) fn has_value(res: &Resolver<'_>, sheet: &SheetRef, addr: &CellAddr) -> bool {
    res.resolve(sheet, addr).is_some_and(|found| found.value.cell.is_some_and(|c| c.value.is_some()))
}

/// Counts of what the commands do, for the unit tests of this module (a count is a bound that does not depend on the speed of the machine).
#[cfg(test)]
pub(crate) mod counts {
    use std::cell::Cell;

    use super::runs::Kind;

    thread_local! {
        /// The passes over a section's rows that took rows out.
        pub(crate) static SECTION_PASSES: Cell<usize> = const { Cell::new(0) };
        /// The rows planned whole by `Edits::set_row`.
        pub(crate) static ROWS_SET: Cell<usize> = const { Cell::new(0) };
        /// The rows of `Character` and of `Paragraph` whose cells were read for a range (`shape.formatting` with `start` and `end`).
        pub(crate) static RUN_ROWS_READ: Cell<usize> = const { Cell::new(0) };
        pub(crate) static PARA_ROWS_READ: Cell<usize> = const { Cell::new(0) };
        /// The parts of a text visited by the reading of a range, for runs and for paragraphs.
        pub(crate) static RUN_PARTS_SCANNED: Cell<usize> = const { Cell::new(0) };
        pub(crate) static PARA_PARTS_SCANNED: Cell<usize> = const { Cell::new(0) };
        /// The times a default face was asked of the font database (`face.rs`).
        pub(crate) static DEFAULTS_LOOKED_UP: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn reset() {
        SECTION_PASSES.with(|c| c.set(0));
        ROWS_SET.with(|c| c.set(0));
        RUN_ROWS_READ.with(|c| c.set(0));
        PARA_ROWS_READ.with(|c| c.set(0));
        RUN_PARTS_SCANNED.with(|c| c.set(0));
        PARA_PARTS_SCANNED.with(|c| c.set(0));
        DEFAULTS_LOOKED_UP.with(|c| c.set(0));
    }

    pub(crate) fn defaults_looked_up() -> usize {
        DEFAULTS_LOOKED_UP.with(Cell::get)
    }

    /// One default face asked of the font database.
    pub(crate) fn default_looked_up() {
        DEFAULTS_LOOKED_UP.with(|c| c.set(c.get() + 1));
    }

    pub(crate) fn section_passes() -> usize {
        SECTION_PASSES.with(Cell::get)
    }

    pub(crate) fn rows_set() -> usize {
        ROWS_SET.with(Cell::get)
    }

    pub(crate) fn run_rows_read() -> usize {
        RUN_ROWS_READ.with(Cell::get)
    }

    pub(crate) fn para_rows_read() -> usize {
        PARA_ROWS_READ.with(Cell::get)
    }

    pub(crate) fn run_parts_scanned() -> usize {
        RUN_PARTS_SCANNED.with(Cell::get)
    }

    /// One part visited by the reading of a range.
    pub(crate) fn scanned(kind: Kind) {
        let counter = match kind {
            Kind::Run => &RUN_PARTS_SCANNED,
            Kind::Paragraph => &PARA_PARTS_SCANNED,
        };
        counter.with(|c| c.set(c.get() + 1));
    }

    /// One row read for a range.
    pub(crate) fn row_read(kind: Kind) {
        let counter = match kind {
            Kind::Run => &RUN_ROWS_READ,
            Kind::Paragraph => &PARA_ROWS_READ,
        };
        counter.with(|c| c.set(c.get() + 1));
    }
}
