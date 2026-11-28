//! Every reference to a shape's connection rows, kept when rows move or go (design §6).
//!
//! A row of a shape's `Connection` section is named in two places: **the `Connect` rows of the page** (point glue, `ToPart` 100 plus the row's `IX` with `ToCell`
//! `Connections.X<IX + 1>`, and the glue of a shape's own outward point, whose `FromPart` is 100 or more) and **the formulas** that say `Connections.X5`,
//! `Sheet.9!Connections.DirY12` or `Connections.Row_3.X`, among them the two cells of a glued connector's end. A command that removes a row, or lets one move up to
//! close the gap, gives a [`RowMap`], which says for each old row what became of it, and these functions bring every reference along:
//!
//! * [`renumber_page`] walks every cell of every shape of the page (members of groups too) and the page's own sheet **once for the call**, and rewrites the number or
//!   name of each reference to a row of the shape in the formula text, character for character elsewhere (`diacraft_calc::row_refs`). It does not touch a
//!   trigger (`_XFTRIGGER(Sheet.5!EventXFMod)` names the shape, no row), skips a formula with no `Connections` in it after one byte scan, and is not made at all for a
//!   map that moves nothing (a doubling only appends rows). The shape's marker `User.DiaCraftAddedPoints` is a list and not a reference to follow: its terms of
//!   moved rows are renumbered and its terms of removed rows dropped, never mapped to a successor (`conn_marker`).
//! * [`renumber_connects`] renumbers the `Connect` rows, the part and the cell together.
//!
//! **A removed row's references go to its successor**, the remaining row the caller chose for it (design §6.3), and an end that must choose for itself (two
//! connectors on one removed point, one arriving from the left and one from the right) says so with [`RowMap::glue_end`]. A row with no successor that something
//! names is refused in words.
//!
//! **Hostile pages.** The walk is bounded by a [`ScanWork`]: the formulas it looks at ([`MAX_SCAN_CELLS`]) and their bytes ([`MAX_SCAN_BYTES`]), both counted as it goes
//! and refused in words when passed, and the `Connect` rows it rewrites ([`MAX_RENUMBERED_ENDS`]). It plans every edit before it makes one, and copies only what it
//! changes (a shape that names no row that moved keeps its sheet).

use std::cell::Cell as Flag;
use std::collections::HashMap;
use std::sync::Arc;

use diacraft_calc::row_refs::{self, CONNECTIONS, RowName, RowRefError, RowTarget};
use diacraft_doc::{Cell, CellAddr, CellName, Connect, Connects, Document, FormulaSlot, MAX_SHAPE_DEPTH, Row, Section, ShapeList, ShapeRef, Sheet};

use super::conn_marker::{self, Found, MARKER_ROW};
use super::connectors::TO_PART_BASE;
use super::{Problem, Which, page_mut, require_page, require_shape, shape_mut};

/// The most formula cells one call looks at, over the page and its shapes.
pub const MAX_SCAN_CELLS: usize = 4_000_000;

/// The most bytes of formula text one call scans.
pub const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;

/// The most `Connect` rows one call rewrites: the glued ends of one shape (as Change Shape's `MAX_RECONNECTED_ENDS`).
pub const MAX_RENUMBERED_ENDS: usize = 4_096;

/// The most rows a [`RowMap`] holds: a section's rows at most.
pub const MAX_MAPPED_ROWS: usize = 65_536;

/// What a refusal of the walk says when it ran past its cells or its bytes.
const TOO_MANY: &str = "the page holds too many formulas to look through for references to the points";

/// The work of one call, counted as it is done, and its caps.
#[derive(Clone, Debug)]
pub struct ScanWork {
    /// The formula cells looked at: every formula of the page, one each.
    pub cells: usize,
    /// The bytes of formula text passed over, the pass that knows a formula names the section, the scan of the ones that do, and the bytes written.
    pub bytes: usize,
    /// The `Connect` rows rewritten.
    pub ends: usize,
    max_cells: usize,
    max_bytes: usize,
    max_ends: usize,
}

impl ScanWork {
    /// A call's work with the caps of the design: [`MAX_SCAN_CELLS`], [`MAX_SCAN_BYTES`] and [`MAX_RENUMBERED_ENDS`].
    pub fn new() -> ScanWork {
        ScanWork::limited(MAX_SCAN_CELLS, MAX_SCAN_BYTES, MAX_RENUMBERED_ENDS)
    }

    /// A call's work with caps of its own, lower than the design's: for a caller that holds a tighter limit, and the tests of what a refusal says.
    pub fn limited(max_cells: usize, max_bytes: usize, max_ends: usize) -> ScanWork {
        ScanWork { cells: 0, bytes: 0, ends: 0, max_cells, max_bytes, max_ends }
    }

    /// Charges the look at one formula cell of `bytes` bytes: refused, with nothing charged, when the cell or its bytes are past the caps.
    fn charge_cell(&mut self, bytes: usize) -> Result<(), Problem> {
        if self.cells >= self.max_cells || bytes > self.bytes_left() {
            return Err(Problem::failed(TOO_MANY));
        }
        self.cells = self.cells.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    fn bytes_left(&self) -> usize {
        self.max_bytes.saturating_sub(self.bytes)
    }

    /// Charges one `Connect` row to be rewritten for shape `shape`.
    fn charge_end(&mut self, shape: u32) -> Result<(), Problem> {
        if self.ends >= self.max_ends {
            return Err(Problem::failed(format!("more than {} connector ends are glued to the points of shape {shape}", self.max_ends)));
        }
        self.ends = self.ends.saturating_add(1);
        Ok(())
    }
}

impl Default for ScanWork {
    fn default() -> ScanWork {
        ScanWork::new()
    }
}

/// The number a formula gives the row at `index`: its `IX` plus one. `None` for the largest `IX`, whose number would not fit.
pub fn row_number(index: u32) -> Option<u32> {
    index.checked_add(1)
}

/// The `ToPart` of the row at `index`: 100 plus its `IX`. `None` where it would not fit.
pub fn part_of(index: u32) -> Option<u32> {
    TO_PART_BASE.checked_add(index)
}

/// The `IX` a part of 100 or more names: the inverse of [`part_of`]. `None` for a part below 100, which names no row.
pub fn index_of(part: u32) -> Option<u32> {
    part.checked_sub(TO_PART_BASE)
}

/// A row as it stands after the change: its `IX` (the number `ToPart` is 100 plus and `Connections.X<n>` names less one), and its name when it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub index: u32,
    pub name: Option<String>,
}

impl Place {
    /// How a formula names the row: by its name when it has one, else by its number.
    fn row_name(&self) -> RowName {
        match &self.name {
            Some(name) => RowName::Name(name.clone()),
            None => RowName::Number(row_number(self.index).unwrap_or(0)),
        }
    }
}

/// What became of one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    /// The row stays, at this `IX` (the same one, or one less because rows before it went).
    Kept(u32),
    /// The row goes. Its references go to the successor the caller chose for it, or to none when the shape keeps no point (a reference that something names is then
    /// refused).
    Removed(Option<Place>),
}

impl Fate {
    /// A removed row whose successor is the row now at `index`, which has no name.
    pub fn removed_to(index: u32) -> Fate {
        Fate::Removed(Some(Place { index, name: None }))
    }
}

/// One old row of the map: its `IX` (or its place, for a section keyed by name), its name if it has one, and its fate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowEntry {
    pub index: u32,
    pub name: Option<String>,
    pub fate: Fate,
}

#[cfg(test)]
thread_local! {
    /// The looks [`RowMap::entry_named`] has made in its maps of names on this thread (at most two for a reference, whatever the number of rows of the name): what the
    /// tests that count the cost of a named reference read.
    static NAMED_LOOKS: Flag<usize> = const { Flag::new(0) };
}

/// What a reference to a row becomes.
enum Mapped {
    Leave,
    To(RowName),
    Gone,
}

/// What a glued end's row becomes.
enum Moved {
    Same,
    To(u32),
    Gone,
}

/// For every row of a shape's `Connection` section before a change, what became of it (design §6.3). Rows the map does not list are rows the shape never had.
#[derive(Clone, Debug)]
pub struct RowMap {
    /// The rows by old `IX`.
    entries: Vec<RowEntry>,
    /// The place in `entries` of the lowest row of each name as the row spells it, and (`folded`) of each name in lower case, so that a name is found in any case with the
    /// exact spelling and then the lower `IX` winning, by one look at each and not a scan of the rows that share the name.
    exact: HashMap<String, usize>,
    folded: HashMap<String, usize>,
    /// The row an end of a connector chose for itself when its own row goes: by connector and by the begin (true) or the end (false), the row's new `IX`.
    ends: HashMap<(u32, bool), u32>,
    identity: bool,
}

impl RowMap {
    /// A map of these rows. Refused in words for more than [`MAX_MAPPED_ROWS`] and for one row listed twice.
    pub fn new(mut entries: Vec<RowEntry>) -> Result<RowMap, Problem> {
        if entries.len() > MAX_MAPPED_ROWS {
            return Err(Problem::bad(format!("the row map holds more than {MAX_MAPPED_ROWS} rows")));
        }
        entries.sort_by_key(|e| e.index);
        if let Some(twice) = entries.windows(2).find_map(|pair| match pair {
            [a, b] if a.index == b.index => Some(a.index),
            _ => None,
        }) {
            return Err(Problem::bad(format!("the row map names row {twice} twice")));
        }
        let identity = entries.iter().all(|e| matches!(e.fate, Fate::Kept(to) if to == e.index));
        // The rows are in `IX` order, so the first row of a name is the lowest.
        let (mut exact, mut folded): (HashMap<String, usize>, HashMap<String, usize>) = (HashMap::new(), HashMap::new());
        for (at, name) in entries.iter().enumerate().filter_map(|(at, e)| e.name.as_ref().map(|name| (at, name))) {
            exact.entry(name.clone()).or_insert(at);
            folded.entry(name.to_ascii_lowercase()).or_insert(at);
        }
        Ok(RowMap { entries, exact, folded, ends: HashMap::new(), identity })
    }

    /// Whether no row moves or goes: a doubling, which only appends. No reference changes, and the walk is not made.
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    /// The rows of the map, by old `IX`.
    pub fn rows(&self) -> &[RowEntry] {
        &self.entries
    }

    /// The end `which` of connector `connector`, glued to a row that goes, is glued to the row now at `to` and not to the successor of the row (two connectors on
    /// one removed point, each to the neighbour on its own side). It says nothing of a row that stays.
    pub fn glue_end(&mut self, connector: u32, which: Which, to: u32) {
        self.ends.insert((connector, which == Which::Begin), to);
    }

    fn entry_at(&self, index: u32) -> Option<&RowEntry> {
        self.entries.binary_search_by_key(&index, |e| e.index).ok().and_then(|at| self.entries.get(at))
    }

    /// The row named `name` in any letter case: the exact spelling first, then the lowest `IX`. One look at the exact spellings, and one at the spellings in lower
    /// case, however many rows share the name: a section may hold 65,536 rows of one name, and a formula may name it in any case as often as its bytes allow.
    fn entry_named(&self, name: &str) -> Option<&RowEntry> {
        let exact = self.exact.get(name);
        #[cfg(test)]
        NAMED_LOOKS.with(|n| n.set(n.get().saturating_add(1)));
        let at = match exact {
            Some(at) => Some(at),
            None => {
                #[cfg(test)]
                NAMED_LOOKS.with(|n| n.set(n.get().saturating_add(1)));
                // A name with no capital is its own lower case; any other costs the copy of the name the formula's scan has just been charged for.
                if name.bytes().any(|b| b.is_ascii_uppercase()) { self.folded.get(&name.to_ascii_lowercase()) } else { self.folded.get(name) }
            }
        };
        at.and_then(|at| self.entries.get(*at))
    }

    /// The entry a reference to `row` names: by number, by name (a bare reference is row 1).
    fn entry_of(&self, row: &RowName) -> Option<&RowEntry> {
        match row {
            RowName::Number(n) => n.checked_sub(1).and_then(|ix| self.entry_at(ix)),
            RowName::Name(name) => self.entry_named(name),
            RowName::Default => self.entry_at(0),
        }
    }

    /// The row an end of connector `end.0` chose for itself, if it did.
    fn chosen(&self, end: Option<(u32, Which)>) -> Option<u32> {
        let (connector, which) = end?;
        self.ends.get(&(connector, which == Which::Begin)).copied()
    }

    /// What a reference in a formula to `row` becomes. A row that stays keeps its name when it moves; a row that goes takes the end's own choice if the formula is
    /// a cell of an end that made one, else its successor's.
    fn formula_row(&self, row: &RowName, end: Option<(u32, Which)>) -> Mapped {
        let Some(entry) = self.entry_of(row) else { return Mapped::Leave };
        match &entry.fate {
            Fate::Kept(to) if *to == entry.index => Mapped::Leave,
            Fate::Kept(to) => match (row, row_number(*to)) {
                (RowName::Name(_), _) | (_, None) => Mapped::Leave,
                (_, Some(number)) => Mapped::To(RowName::Number(number)),
            },
            Fate::Removed(successor) => match (self.chosen(end), successor) {
                (Some(to), _) => row_number(to).map_or(Mapped::Gone, |number| Mapped::To(RowName::Number(number))),
                (None, Some(place)) => Mapped::To(place.row_name()),
                (None, None) => Mapped::Gone,
            },
        }
    }

    /// What the row at `index` (a `ToPart` less 100) becomes for a glued end.
    fn part_row(&self, index: u32, end: Option<(u32, Which)>) -> Moved {
        let Some(entry) = self.entry_at(index) else { return Moved::Same };
        match &entry.fate {
            Fate::Kept(to) if *to == entry.index => Moved::Same,
            Fate::Kept(to) => Moved::To(*to),
            Fate::Removed(successor) => match (self.chosen(end), successor) {
                (Some(to), _) => Moved::To(to),
                (None, Some(place)) => Moved::To(place.index),
                (None, None) => Moved::Gone,
            },
        }
    }

    /// `row` as the marker's reading has it: the spelling the map has it in (a number as written, a name as the row spells it) and the row it is, by the `IX` the map gives
    /// it, so that `Connections.X1` and `Connections.Tally.X` are the one row when row 0 is named `Tally`.
    fn canonical(&self, row: &RowName) -> Option<Found> {
        let entry = self.entry_of(row)?;
        let spelling = match row {
            RowName::Number(n) => RowName::Number(*n),
            RowName::Name(_) => RowName::Name(entry.name.clone()?),
            RowName::Default => return None,
        };
        Some(Found { spelling, identity: entry.index })
    }

    /// The row a marker's term names after the change: the same row at its new number, or `None` for a row that goes (its term is dropped, never mapped).
    fn marker_row(&self, row: &RowName) -> Option<RowName> {
        let entry = self.entry_of(row)?;
        match (&entry.fate, row) {
            (Fate::Kept(_), RowName::Name(_)) => Some(row.clone()),
            (Fate::Kept(to), _) => row_number(*to).map(RowName::Number),
            (Fate::Removed(_), _) => None,
        }
    }
}

/// What [`renumber_page`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Renumbered {
    /// The formula cells rewritten, not counting the marker.
    pub formulas: usize,
    /// Of those, the cells of a connector's end (`BeginX`, `BeginY`, `EndX`, `EndY`): the glue's own formulas, which a caller counts apart from the other references.
    pub end_cells: usize,
    /// The marker's terms renumbered.
    pub terms_moved: usize,
    /// The marker's terms dropped: those of removed rows, and those that named no row, a row twice or a generation of 0.
    pub terms_dropped: usize,
}

/// One end of a connector that went to another row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndMoved {
    /// The connector (the `Connect` row's `FromSheet`).
    pub connector: u32,
    /// Which end, by the cell the row names; `None` for a row that names another.
    pub which: Option<Which>,
    /// The shape whose point it is glued to.
    pub shape: u32,
    /// The row's `IX` before and after.
    pub from: u32,
    pub to: u32,
}

/// What [`renumber_connects`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Connected {
    /// The ends glued to a row of the shape that is now glued to another, in the page's order.
    pub ends: Vec<EndMoved>,
    /// The `Connect` rows of the shape's own outward points (`FromPart` 100 or more) that were rewritten.
    pub own_side: usize,
}

/// What a text became under a map.
enum Text {
    Same,
    New(String),
    Gone,
}

/// A text renumbered by `map` for `target`: the bytes it takes are charged to `work`.
fn renumber_text(map: &RowMap, work: &mut ScanWork, text: &str, target: RowTarget, end: Option<(u32, Which)>) -> Result<Text, RowRefError> {
    let gone = Flag::new(false);
    let rewrite = |row: &RowName| match map.formula_row(row, end) {
        Mapped::Leave => None,
        Mapped::To(row) => Some(row),
        Mapped::Gone => {
            gone.set(true);
            None
        }
    };
    let mut budget = work.bytes_left();
    let before = budget;
    let result = row_refs::renumber(text, CONNECTIONS, target, &rewrite, &mut budget);
    work.bytes = work.bytes.saturating_add(before.saturating_sub(budget));
    let renumbered = result?;
    Ok(match (gone.get(), renumbered) {
        (true, _) => Text::Gone,
        (false, Some(new)) => Text::New(new),
        (false, None) => Text::Same,
    })
}

/// The refusal of a row that goes with nothing to take its references.
fn last_point_words(shape: u32, named_by: &str) -> String {
    format!("halving would remove the last connection point of shape {shape}, which {named_by} names")
}

/// What a failed rewrite says, for the formula of `of`.
fn refused(error: RowRefError, shape: u32, of: &str) -> Problem {
    match error {
        RowRefError::BudgetSpent => Problem::failed(TOO_MANY),
        RowRefError::NumberTooLarge { .. } => {
            Problem::failed(format!("the formula of {of} names a connection point of shape {shape} by a number past {}", u32::MAX))
        }
        other => Problem::failed(format!("the formula of {of} cannot be renumbered: {other}")),
    }
}

/// Where a cell is in a sheet, to find it again when it is written.
#[derive(Clone, Copy, Debug)]
enum Spot {
    Direct,
    Section(usize),
    Row(usize, usize),
}

/// A cell's place and its section and row, for the words of a refusal and the marker's skip.
struct At<'a> {
    spot: Spot,
    section: Option<&'a Section>,
    row: Option<&'a Row>,
}

impl At<'_> {
    /// Whether this is the cell `User.DiaCraftAddedPoints.Value`.
    fn is_marker(&self, name: &CellName) -> bool {
        name.as_str() == "Value"
            && self.section.is_some_and(|s| s.name.as_str() == "User")
            && self.row.is_some_and(|r| r.name.as_deref() == Some(MARKER_ROW))
    }

    /// The cell as a reference spells it, for the words of a refusal.
    fn address(&self, name: &CellName) -> CellAddr {
        let direct = || CellAddr::direct(name.as_str());
        match (self.section, self.row) {
            (None, _) => direct(),
            (Some(section), None) => CellAddr::in_section(section.name.as_str(), section.ix, name.as_str()),
            (Some(section), Some(row)) => match row.key(section.name.as_str()) {
                Some(key) => CellAddr::in_row(section.name.as_str(), section.ix, key, name.as_str()),
                None => direct(),
            },
        }
    }
}

/// A formula to write, and where.
struct Edit {
    spot: Spot,
    name: CellName,
    formula: String,
}

/// The edits of a whole page, found before any is made.
#[derive(Default)]
struct Plan {
    /// By the path of the shape.
    shapes: Vec<(Vec<u32>, Vec<Edit>)>,
    page: Vec<Edit>,
    formulas: usize,
    end_cells: usize,
}

/// The walk over a page's formulas: whose rows moved, how, and what it has cost.
struct Walk<'a> {
    map: &'a RowMap,
    target: u32,
    work: &'a mut ScanWork,
    /// The cells of a connector's end that were rewritten.
    end_cells: usize,
}

impl Walk<'_> {
    /// The formulas of one sheet that name a row that moves, rewritten. `owner` is the shape whose sheet it is, `None` for the page's.
    fn sheet(&mut self, sheet: &Sheet, owner: Option<u32>) -> Result<Vec<Edit>, Problem> {
        let mut edits = Vec::new();
        for (name, cell) in &sheet.cells {
            self.cell(&mut edits, &At { spot: Spot::Direct, section: None, row: None }, name, cell, owner)?;
        }
        for (si, section) in sheet.sections.iter().enumerate() {
            if section.del {
                continue;
            }
            for (name, cell) in &section.cells {
                self.cell(&mut edits, &At { spot: Spot::Section(si), section: Some(section.as_ref()), row: None }, name, cell, owner)?;
            }
            for (ri, row) in section.rows.iter().enumerate() {
                if row.del {
                    continue;
                }
                for (name, cell) in &row.cells {
                    self.cell(
                        &mut edits,
                        &At { spot: Spot::Row(si, ri), section: Some(section.as_ref()), row: Some(row.as_ref()) },
                        name,
                        cell,
                        owner,
                    )?;
                }
            }
        }
        Ok(edits)
    }

    /// One cell: counted, and its formula rewritten if it names a row of the shape that moves.
    fn cell(&mut self, edits: &mut Vec<Edit>, at: &At<'_>, name: &CellName, cell: &Cell, owner: Option<u32>) -> Result<(), Problem> {
        let FormulaSlot::Local(formula) = &cell.formula else { return Ok(()) };
        let own = owner == Some(self.target);
        // The marker is a list, which the renumbering of the page reads on its own.
        if own && at.is_marker(name) {
            return Ok(());
        }
        let text: &str = &formula.source;
        self.work.charge_cell(text.len())?;
        if !row_refs::mentions(text, CONNECTIONS) {
            return Ok(());
        }
        // The cells of an end of a connector are the ones an end that chose its own row speaks for.
        let end = match (at.spot, owner, name.as_str()) {
            (Spot::Direct, Some(id), "BeginX" | "BeginY") => Some((id, Which::Begin)),
            (Spot::Direct, Some(id), "EndX" | "EndY") => Some((id, Which::End)),
            _ => None,
        };
        let target = RowTarget { sheet: self.target, unqualified: own };
        let rewritten = renumber_text(self.map, self.work, text, target, end);
        let of = || {
            let reference = diacraft_calc::reference_name(&at.address(name)).unwrap_or_else(|| name.to_string());
            let reference = match (&at.section, reference.strip_suffix(".Value")) {
                (Some(section), Some(short)) if section.name.as_str() == "User" => short.to_owned(),
                _ => reference,
            };
            match owner {
                Some(id) => format!("Sheet.{id}!{reference}"),
                None => format!("the page's {reference}"),
            }
        };
        match rewritten {
            Ok(Text::Same) => {}
            Ok(Text::New(formula)) => {
                self.end_cells = self.end_cells.saturating_add(usize::from(end.is_some()));
                edits.push(Edit { spot: at.spot, name: name.clone(), formula });
            }
            Ok(Text::Gone) => return Err(Problem::failed(last_point_words(self.target, &format!("the formula of {}", of())))),
            Err(error) => return Err(refused(error, self.target, &of())),
        }
        Ok(())
    }
}

/// The edits of the page's formulas, shapes in the order they stand (a group, then its members), then the page's own sheet.
fn plan_page(doc: &Document, page: u32, shape: u32, map: &RowMap, work: &mut ScanWork) -> Result<Plan, Problem> {
    let Some(p) = doc.page(page) else { return Err(Problem::bad(format!("there is no page {page}"))) };
    let mut walk = Walk { map, target: shape, work, end_cells: 0 };
    let mut plan = Plan::default();
    let mut frames: Vec<(usize, &ShapeList)> = vec![(0, &p.shapes)];
    // The indices of the groups the walk is in.
    let mut path: Vec<u32> = Vec::new();
    while let Some(frame) = frames.last_mut() {
        let (next, list) = (frame.0, frame.1);
        let Some(s) = list.get(next) else {
            frames.pop();
            path.pop();
            continue;
        };
        frame.0 = next.saturating_add(1);
        let here = u32::try_from(next).map_err(|_| Problem::failed("a page holds more shapes than a path can number"))?;
        let edits = walk.sheet(&s.sheet, Some(s.id))?;
        if !edits.is_empty() {
            let mut at = path.clone();
            at.push(here);
            plan.formulas = plan.formulas.saturating_add(edits.len());
            plan.shapes.push((at, edits));
        }
        if !s.children.is_empty() && frames.len() <= MAX_SHAPE_DEPTH {
            path.push(here);
            frames.push((0, &s.children));
        }
    }
    let edits = walk.sheet(&p.sheet, None)?;
    plan.formulas = plan.formulas.saturating_add(edits.len());
    plan.page = edits;
    plan.end_cells = walk.end_cells;
    Ok(plan)
}

/// Puts the rewritten formulas into a sheet, cell by cell and in place, keeping everything else of each cell: its value, unit, error and extras.
fn write_edits(sheet: &mut Sheet, edits: Vec<Edit>) {
    for edit in edits {
        let cell = match edit.spot {
            Spot::Direct => sheet.cells.get_mut(&edit.name),
            Spot::Section(si) => sheet.sections.get_mut(si).and_then(|s| Arc::make_mut(s).cells.get_mut(&edit.name)),
            Spot::Row(si, ri) => {
                sheet.sections.get_mut(si).and_then(|s| Arc::make_mut(s).rows.get_mut(ri)).and_then(|r| Arc::make_mut(r).cells.get_mut(&edit.name))
            }
        };
        if let Some(cell) = cell {
            cell.formula = Cell::with_formula(&edit.formula, None).formula;
        }
    }
}

/// Brings the formulas of the page that name rows of shape `shape` along with `map`, and the shape's marker (design §6.4, §6.5). Every formula cell of every shape
/// of page `page`, members too, and the page's own sheet, is looked at once, and only a cell whose text names a row that moves is rewritten, by the number or name
/// of the row and no other byte. The shape's own unqualified references name its own rows; those of any other sheet name them by `Sheet.<id>!`.
///
/// A map that moves nothing costs nothing: the walk is not made, and the marker is left (a doubling writes its own terms: `Marker::with_terms` and `conn_marker::write`).
/// A refusal (a page past `work`'s caps, a row that goes and that a formula names with no successor to take it) is in words and changes nothing: nothing is written
/// before every cell has been looked at.
///
/// **The marker is read against the map, and not against the shape's rows**: a term stands when the map lists its row, so the call gives the same answer whether the
/// shape's `Connection` section has been rewritten already or not. A term that names no row of the map is void, as a term of a deleted row is.
pub fn renumber_page(doc: &mut Document, page: u32, shape: u32, map: &RowMap, work: &mut ScanWork) -> Result<Renumbered, Problem> {
    require_page(doc, page)?;
    let target = require_shape(doc, page, shape)?;
    if map.is_identity() {
        return Ok(Renumbered::default());
    }
    let plan = plan_page(doc, page, shape, map, work)?;
    let mut done = Renumbered { formulas: plan.formulas, end_cells: plan.end_cells, ..Renumbered::default() };
    for (path, edits) in plan.shapes {
        let changed = shape_mut(doc, page, &path).ok_or_else(|| Problem::failed("a shape went away while the page was renumbered"))?;
        write_edits(Arc::make_mut(&mut changed.sheet), edits);
    }
    if !plan.page.is_empty() {
        let changed = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away while it was renumbered"))?;
        write_edits(Arc::make_mut(&mut changed.sheet), plan.page);
    }
    renumber_marker(doc, &target, map, &mut done)?;
    Ok(done)
}

/// The marker of the shape as a list: moved rows' terms renumbered, removed rows' dropped, and the terms that were void to begin with dropped. A marker of another
/// form is left as it is: no reader takes it for ours.
fn renumber_marker(doc: &mut Document, at: &ShapeRef, map: &RowMap, done: &mut Renumbered) -> Result<(), Problem> {
    let Some(text) = doc.shape(at).and_then(|shape| conn_marker::local_formula(&shape.sheet)) else { return Ok(()) };
    let Some((marker, total)) = conn_marker::parse_counted(&text, &|row: &RowName| map.canonical(row)) else { return Ok(()) };
    let moved = Flag::new(0_usize);
    let renumbered = marker.renumbered(&|row: &RowName| {
        let new = map.marker_row(row);
        if new.as_ref().is_some_and(|n| n != row) {
            moved.set(moved.get().saturating_add(1));
        }
        new
    });
    if renumbered.text().as_deref() == Some(text.as_str()) {
        return Ok(());
    }
    done.terms_moved = moved.get();
    done.terms_dropped = total.saturating_sub(renumbered.len());
    conn_marker::write(doc, at, &renumbered)
}

/// Which end a `Connect` row is of, by the cell it names.
fn which_of(from_cell: Option<&str>) -> Option<Which> {
    match from_cell {
        Some("BeginX" | "BeginY") => Some(Which::Begin),
        Some("EndX" | "EndY") => Some(Which::End),
        _ => None,
    }
}

/// One side of a `Connect` row that names a row of the shape, after it is renumbered: its part and its cell together, and the row's `IX` before and after.
struct Side {
    part: u32,
    cell: Option<Arc<str>>,
    from: u32,
    to: u32,
}

/// One side of a `Connect` row renumbered by `map`: `None` when neither its part nor its cell changes.
fn move_side(
    part: u32,
    cell: Option<&Arc<str>>,
    shape: u32,
    map: &RowMap,
    end: Option<(u32, Which)>,
    work: &mut ScanWork,
    by: u32,
) -> Result<Option<Side>, Problem> {
    let named_by = || format!("a Connect row of shape {by}");
    let Some(from) = index_of(part) else { return Ok(None) };
    let to = match map.part_row(from, end) {
        Moved::Same => from,
        Moved::To(to) => to,
        Moved::Gone => return Err(Problem::failed(last_point_words(shape, &named_by()))),
    };
    let renumbered = match cell {
        Some(text) => match renumber_text(map, work, text, RowTarget::own(shape), end) {
            Ok(Text::New(new)) => Some(Arc::<str>::from(new)),
            Ok(Text::Same) => None,
            Ok(Text::Gone) => return Err(Problem::failed(last_point_words(shape, &named_by()))),
            Err(error) => return Err(refused(error, shape, &named_by())),
        },
        None => None,
    };
    if to == from && renumbered.is_none() {
        return Ok(None);
    }
    let part = part_of(to).ok_or_else(|| Problem::failed("a connection point's number is past what a part can hold"))?;
    Ok(Some(Side { part, cell: renumbered.or_else(|| cell.cloned()), from, to }))
}

/// Renumbers the `Connect` rows of page `page` that name a row of shape `shape`, the part and the cell together: a row whose `ToSheet` is the shape and whose
/// `ToPart` is 100 or more (a connector glued to one of its points), and a row whose `FromSheet` is the shape and whose `FromPart` is 100 or more (the glue of its own
/// outward point to another shape). The other side of such a row and every other row are left as they are. At most [`MAX_RENUMBERED_ENDS`] rows are rewritten
/// (`work`), and a row that goes with no successor is refused in words. A map that moves nothing is not looked at.
pub fn renumber_connects(doc: &mut Document, page: u32, shape: u32, map: &RowMap, work: &mut ScanWork) -> Result<Connected, Problem> {
    require_page(doc, page)?;
    let mut out = Connected::default();
    if map.is_identity() {
        return Ok(out);
    }
    let Some(p) = doc.page(page) else { return Err(Problem::bad(format!("there is no page {page}"))) };
    let mut changes: Vec<(usize, Connect)> = Vec::new();
    for (at, c) in p.connects.iter().enumerate() {
        let names_row = |part: Option<u32>| part.is_some_and(|part| part >= TO_PART_BASE);
        let (to_side, own_side) = (c.to_sheet == shape && names_row(c.to_part), c.from_sheet == shape && names_row(c.from_part));
        if !to_side && !own_side {
            continue;
        }
        let end = which_of(c.from_cell.as_deref()).map(|which| (c.from_sheet, which));
        let mut next: Option<Connect> = None;
        if to_side
            && let Some(part) = c.to_part
            && let Some(side) = move_side(part, c.to_cell.as_ref(), shape, map, end, work, c.from_sheet)?
        {
            let row = next.get_or_insert_with(|| c.clone());
            (row.to_part, row.to_cell) = (Some(side.part), side.cell);
            if side.from != side.to {
                out.ends.push(EndMoved { connector: c.from_sheet, which: end.map(|(_, which)| which), shape, from: side.from, to: side.to });
            }
        }
        if own_side
            && let Some(part) = c.from_part
            && let Some(side) = move_side(part, c.from_cell.as_ref(), shape, map, None, work, c.from_sheet)?
        {
            let row = next.get_or_insert_with(|| c.clone());
            (row.from_part, row.from_cell) = (Some(side.part), side.cell);
            out.own_side = out.own_side.saturating_add(1);
        }
        if let Some(row) = next {
            work.charge_end(shape)?;
            changes.push((at, row));
        }
    }
    if !changes.is_empty() {
        let changed = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away while it was renumbered"))?;
        let mut list: Vec<Connect> = changed.connects.iter().cloned().collect();
        for (at, next) in changes {
            if let Some(slot) = list.get_mut(at) {
                *slot = next;
            }
        }
        changed.connects = Connects::from_vec(list);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn named(index: u32, name: &str, fate: Fate) -> RowEntry {
        RowEntry { index, name: Some(name.into()), fate }
    }

    #[test]
    fn a_name_is_found_in_any_case_the_exact_spelling_first_and_then_the_lower_row() {
        let map = RowMap::new(vec![named(0, "Tally", Fate::Kept(0)), named(1, "TALLY", Fate::Kept(1)), named(2, "other", Fate::Kept(2))]).unwrap();
        assert_eq!(map.entry_named("TALLY").map(|e| e.index), Some(1));
        assert_eq!(map.entry_named("Tally").map(|e| e.index), Some(0));
        assert_eq!(map.entry_named("tally").map(|e| e.index), Some(0), "no exact spelling: the lower row");
        assert_eq!(map.entry_named("OTHER").map(|e| e.index), Some(2));
        assert_eq!(map.entry_named("nothing").map(|e| e.index), None);
        assert_eq!(map.entry_named("").map(|e| e.index), None);
    }

    #[test]
    fn the_exact_spelling_and_the_lowest_row_win_among_rows_that_share_a_name() {
        let map = RowMap::new(vec![
            named(0, "tally", Fate::Kept(0)),
            named(1, "Tally", Fate::Kept(1)),
            named(2, "Tally", Fate::Kept(2)),
            named(3, "TALLY", Fate::Kept(3)),
            named(4, "TALLY", Fate::Kept(4)),
        ])
        .unwrap();
        let at = |name: &str| map.entry_named(name).map(|e| e.index);
        assert_eq!(at("Tally"), Some(1), "the lowest row of the exact spelling");
        assert_eq!(at("TALLY"), Some(3));
        assert_eq!(at("tally"), Some(0));
        assert_eq!(at("tAlLy"), Some(0), "no exact spelling: the lowest row of any case");
        assert_eq!(at("tall"), None);
        assert_eq!(at("tallyx"), None);
    }

    #[test]
    fn a_reference_to_a_name_that_65536_rows_share_looks_at_one_row_or_two_in_any_letter_case() {
        let rows: Vec<RowEntry> = (0..65_536).map(|i| named(i, "Tally", Fate::Kept(i))).collect();
        let map = RowMap::new(rows).unwrap();
        let spellings = ["TALLY", "tally", "tAlLy", "Tally", "TaLLY", "nothing", "Tall"];
        NAMED_LOOKS.with(|n| n.set(0));
        for _ in 0..200 {
            for spelling in spellings {
                let found = map.entry_named(spelling).map(|e| e.index);
                assert_eq!(found, if spelling.eq_ignore_ascii_case("tally") { Some(0) } else { None }, "{spelling}");
            }
        }
        let looks = NAMED_LOOKS.with(Flag::get);
        assert!(looks <= 2 * 200 * spellings.len(), "{looks} rows looked at for {} references", 200 * spellings.len());
    }

    #[test]
    fn a_reference_is_left_renumbered_or_gone_by_what_became_of_its_row() {
        let map = RowMap::new(vec![
            RowEntry { index: 0, name: None, fate: Fate::Kept(0) },
            RowEntry { index: 1, name: None, fate: Fate::removed_to(0) },
            RowEntry { index: 2, name: None, fate: Fate::Kept(1) },
            RowEntry { index: 3, name: None, fate: Fate::Removed(None) },
            named(4, "Row_5", Fate::Kept(2)),
        ])
        .unwrap();
        let to = |row: RowName, end: Option<(u32, Which)>| match map.formula_row(&row, end) {
            Mapped::Leave => "leave".to_owned(),
            Mapped::To(r) => format!("{r:?}"),
            Mapped::Gone => "gone".to_owned(),
        };
        assert_eq!(to(RowName::Number(1), None), "leave");
        assert_eq!(to(RowName::Number(2), None), "Number(1)");
        assert_eq!(to(RowName::Number(3), None), "Number(2)");
        assert_eq!(to(RowName::Number(4), None), "gone");
        assert_eq!(to(RowName::Number(5), None), "Number(3)", "a row by its place is renumbered whether or not it has a name");
        assert_eq!(to(RowName::Name("ROW_5".into()), None), "leave", "a name goes with its row");
        assert_eq!(to(RowName::Number(99), None), "leave", "a row the map does not list is none of its concern");
        assert_eq!(to(RowName::Number(0), None), "leave");
        // The bare reference is row 1.
        assert_eq!(to(RowName::Default, None), "leave");
    }

    #[test]
    fn an_end_that_chose_a_row_takes_it_only_for_a_row_that_goes() {
        let mut map =
            RowMap::new(vec![RowEntry { index: 0, name: None, fate: Fate::removed_to(5) }, RowEntry { index: 1, name: None, fate: Fate::Kept(0) }])
                .unwrap();
        map.glue_end(9, Which::End, 3);
        assert!(matches!(map.part_row(0, Some((9, Which::End))), Moved::To(3)));
        assert!(matches!(map.part_row(0, Some((9, Which::Begin))), Moved::To(5)), "the other end of the connector takes the successor");
        assert!(matches!(map.part_row(0, Some((8, Which::End))), Moved::To(5)), "and so does another connector");
        assert!(matches!(map.part_row(1, Some((9, Which::End))), Moved::To(0)), "a row that stays has no choice to make");
    }

    #[test]
    fn the_marker_keeps_a_moved_rows_term_and_never_maps_a_removed_one() {
        let map =
            RowMap::new(vec![RowEntry { index: 4, name: None, fate: Fate::removed_to(0) }, RowEntry { index: 5, name: None, fate: Fate::Kept(4) }])
                .unwrap();
        assert_eq!(map.marker_row(&RowName::Number(5)), None);
        assert_eq!(map.marker_row(&RowName::Number(6)), Some(RowName::Number(5)));
        assert_eq!(map.marker_row(&RowName::Number(7)), None, "a row the map does not list has no term to keep");
        assert_eq!(map.canonical(&RowName::Number(6)), Some(Found { spelling: RowName::Number(6), identity: 5 }));
        assert_eq!(map.canonical(&RowName::Number(7)), None);
    }
}
