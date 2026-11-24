//! Sheets: the cells, sections and rows that every page, master, shape and style holds, and how one cell is addressed.
//!
//! The shape of a sheet is the file's: cells directly under it (the single-row
//! properties such as `PinX`), and sections, each with cells of its own, and rows, each with cells. Geometry sections repeat
//! and are told apart by `IX`; every other section occurs once, by name. Rows are keyed by `N` or by `IX` as the section
//! says ([`keying`]).
//!
//! **Structural sharing.** A section and a row are behind `Arc`, so writing one cell of one row copies that row, its section's
//! list of rows and the sheet's list of sections, and shares every other row and section with the older copy. A sheet is
//! itself behind an `Arc` in its shape, page, master or style (`shape.rs`), so the older document keeps the old sheet.

use std::collections::{BTreeMap, HashMap};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, PoisonError};

use crate::DocError;
use crate::catalogue::{Family, Keying, keying};
use crate::cell::Cell;
use crate::name::{CellName, SectionName};
use crate::odf::OdfRow;
use crate::package::{Ext, Kept};
use crate::raw::{Passthrough, RawAttrs};

/// Most cells directly under one sheet, section or row that a write may make. **The most the sample-file run found in one place is 258**
/// (lists 462 cell names in all), so this is sixteen times that (it was 1,024). The limit is for writes and
/// builders, and the reader of a file applies it too.
pub const MAX_CELLS_PER_CONTAINER: usize = 4_096;
/// Most sections one sheet may hold: Geometry sections repeat, and a hostile file could repeat them without end. **The most the sample-file run
/// found in one sheet is 726**, so this is eleven times that (it was 1,024).
pub const MAX_SECTIONS_PER_SHEET: usize = 8_192;
/// Most rows one section may hold. **The most the sample-file run found in one section is 31,273** (a `Geometry` section of nearly all
/// `RelLineTo` rows in a 17 MB master part; the next largest real one has 4,040), so this is about ten times that (it
/// was 16,384, which refused 3 real files). A row is found by key in a table built the first time a lookup asks for one in a section of
/// more than [`ROW_SCAN_LIMIT`] rows, never by a scan of them all, so the cap is not what a lookup costs.
pub const MAX_ROWS_PER_SECTION: usize = 320_000;
/// A section of this many rows or fewer is searched by a scan; a larger one has its lookup table.
pub const ROW_SCAN_LIMIT: usize = 32;
/// Longest name of a cell, section or row, in bytes.
pub const MAX_NAME_BYTES: usize = 1_024;

/// Cells by name. Names are exact in case.
pub type Cells = BTreeMap<CellName, Cell>;

/// The three style links of a sheet (`LineStyle`, `FillStyle`, `TextStyle`): each names a style by `ID`, and reaches only the
/// cells of its family. An empty link is no link, so `None`. A link the model could not read as a number
/// is `None` too, with the attribute kept in the sheet's [`RawAttrs`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Links {
    pub line: Option<u32>,
    pub fill: Option<u32>,
    pub text: Option<u32>,
}

impl Links {
    /// The link of one family.
    pub fn get(&self, family: Family) -> Option<u32> {
        match family {
            Family::Line => self.line,
            Family::Fill => self.fill,
            Family::Text => self.text,
        }
    }
}

/// The key of a row within its section.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowKey {
    /// The row's `IX`. It is a key, not a position: rows 0 and 1 may be inherited while the
    /// local section holds only row 2.
    Ix(u32),
    /// The row's `N`.
    Name(Arc<str>),
}

impl RowKey {
    /// A key by name.
    pub fn name(name: &str) -> RowKey {
        RowKey::Name(Arc::from(name))
    }
}

/// A row of a section.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// `N`, in a section keyed by name.
    pub name: Option<Arc<str>>,
    /// `IX`, in a section keyed by index. An absent `IX` is the number of rows before this one:
    /// the reader gives the row that number, so it is a key like any other.
    pub ix: Option<u32>,
    /// `LocalName`: the localised name beside the universal `N`.
    pub local_name: Option<Arc<str>>,
    /// `T`: the row type, in `Geometry` and `Connection` (`MoveTo`, `LineTo`, ...). Verbatim: a type no spec lists is kept.
    pub row_type: Option<Arc<str>>,
    /// `Del`: the row is deleted here, so its inherited values are not.
    pub del: bool,
    pub cells: Cells,
    pub raw: RawAttrs,
    pub passthrough: Passthrough,
    /// The row's ODF data (a glue point held exactly, a paragraph or run's automatic style name) and the VSDX elements and unknown `.diacraft` content it
    /// had no slot for. Filled by the `.odg` reader, the VSDX reader.
    pub ext: Ext<OdfRow>,
}

impl Row {
    /// The row's key in a section called `section`: by `N` where the section is keyed by name, by `IX` where it is keyed by
    /// index, and by `N` when there is one in a section keyed either way. `None` for a row with
    /// neither.
    pub fn key(&self, section: &str) -> Option<RowKey> {
        let by_name = || self.name.as_ref().map(|n| RowKey::Name(Arc::clone(n)));
        let by_index = || self.ix.map(RowKey::Ix);
        match keying(section) {
            Keying::Indexed => by_index().or_else(by_name),
            Keying::Named | Keying::Either => by_name().or_else(by_index),
        }
    }

    fn has_key(&self, keying: Keying, key: &RowKey) -> bool {
        let by_name = |name: &str| self.name.as_deref() == Some(name);
        let by_index = |ix: u32| self.ix == Some(ix);
        match (keying, key) {
            (Keying::Indexed, RowKey::Ix(ix)) => by_index(*ix),
            (Keying::Indexed, RowKey::Name(n)) => self.ix.is_none() && by_name(n),
            (_, RowKey::Name(n)) => by_name(n),
            (_, RowKey::Ix(ix)) => self.name.is_none() && by_index(*ix),
        }
    }
}

/// A section of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// `N`: one of the 17 predefined names, or any other.
    pub name: SectionName,
    /// `IX`, on `Geometry` sections (and only there): their number among the sheet's `Geometry` sections, which a reader
    /// gives an absent one as the count of earlier ones.
    pub ix: Option<u32>,
    /// `Del`: the section is deleted here.
    pub del: bool,
    /// Cells directly under the section: the five flags of a `Geometry` section.
    pub cells: Cells,
    pub rows: Rows,
    pub raw: RawAttrs,
    pub passthrough: Passthrough,
    /// What the element holds that no field reads, tagged by format; see [`crate::package`].
    pub ext: Ext<Kept>,
}

/// The rows of a section: a list that behaves as a `Vec<Arc<Row>>` (it dereferences to one) and holds the lookup table of a large section's
/// rows, so that a row is found by key and not by a scan.
///
/// The table maps each key to the position of the first row that has it. It is built the first time a lookup asks for a row of a section
/// of more than [`ROW_SCAN_LIMIT`] rows, is shared by a clone (the rows are the same at that moment), and is **dropped by any mutable
/// access to the rows** (`push`, `insert`, `truncate`, `get_mut`, a store by index), which is the whole of its correctness: no code can
/// change the list and be answered from a table of the old one. The writes of this crate keep the table as they go ([`Sheet::row_mut`]
/// and its kin), so a run of writes is not a run of rebuilds. A key changed in place through the `&mut Row` of [`Sheet::row_mut`] is the
/// one change the table does not see: put a new row instead.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    list: Vec<Arc<Row>>,
    table: RowTable,
}

impl Rows {
    /// An empty list.
    pub fn new() -> Rows {
        Rows::default()
    }

    /// The table for the rows as they are: the one held, or a new one, kept.
    fn table(&self, keying_of: &str) -> Arc<Table> {
        let mut held = self.table.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(table) = held.as_ref() {
            return Arc::clone(table);
        }
        crate::work::row_table_built();
        let mut keys: HashMap<RowKey, usize> = HashMap::with_capacity(self.list.len());
        for (at, row) in self.list.iter().enumerate() {
            if let Some(key) = row.key(keying_of) {
                // The first row with a key wins, as in a scan.
                keys.entry(key).or_insert(at);
            }
        }
        let table = Arc::new(Table { keys });
        *held = Some(Arc::clone(&table));
        table
    }

    /// Puts `row` after the others, keeping the table (if one is held) as it goes.
    fn push_keeping_table(&mut self, row: Row, section: &str) {
        let key = row.key(section);
        let at = self.list.len();
        self.list.push(Arc::new(row));
        let held = self.table.0.get_mut().unwrap_or_else(PoisonError::into_inner);
        if let Some(table) = held.as_mut()
            && let Some(key) = key
        {
            Arc::make_mut(table).keys.entry(key).or_insert(at);
        }
    }

    /// The row at `at`, to change its cells, without dropping the table: its key is not to be changed through it.
    fn slot_mut(&mut self, at: usize) -> Option<&mut Arc<Row>> {
        self.list.get_mut(at)
    }
}

impl Deref for Rows {
    type Target = Vec<Arc<Row>>;
    fn deref(&self) -> &Vec<Arc<Row>> {
        &self.list
    }
}

impl DerefMut for Rows {
    fn deref_mut(&mut self) -> &mut Vec<Arc<Row>> {
        *self.table.0.get_mut().unwrap_or_else(PoisonError::into_inner) = None;
        &mut self.list
    }
}

impl PartialEq for Rows {
    fn eq(&self, other: &Rows) -> bool {
        self.list == other.list
    }
}

impl From<Vec<Arc<Row>>> for Rows {
    fn from(list: Vec<Arc<Row>>) -> Rows {
        Rows { list, table: RowTable::default() }
    }
}

impl FromIterator<Arc<Row>> for Rows {
    fn from_iter<I: IntoIterator<Item = Arc<Row>>>(rows: I) -> Rows {
        Rows::from(rows.into_iter().collect::<Vec<_>>())
    }
}

impl<'a> IntoIterator for &'a Rows {
    type Item = &'a Arc<Row>;
    type IntoIter = std::slice::Iter<'a, Arc<Row>>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

impl IntoIterator for Rows {
    type Item = Arc<Row>;
    type IntoIter = std::vec::IntoIter<Arc<Row>>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

/// The lookup table behind a [`Rows`]: shared by its clones, and compared as always equal (two lists are equal by their rows).
#[derive(Debug, Default)]
struct RowTable(Mutex<Option<Arc<Table>>>);

#[derive(Clone, Debug)]
struct Table {
    keys: HashMap<RowKey, usize>,
}

impl Clone for RowTable {
    fn clone(&self) -> RowTable {
        RowTable(Mutex::new(self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()))
    }
}

impl Section {
    /// An empty section.
    pub fn new(name: &str, ix: Option<u32>) -> Section {
        Section {
            name: SectionName::new(name),
            ix,
            del: false,
            cells: Cells::new(),
            rows: Rows::new(),
            raw: RawAttrs::new(),
            passthrough: Passthrough::new(),
            ext: Ext::default(),
        }
    }

    /// The first row with this key (a hostile sheet may hold two). A scan of up to [`ROW_SCAN_LIMIT`] rows, and a lookup in the section's
    /// table for more.
    pub fn row(&self, key: &RowKey) -> Option<&Arc<Row>> {
        self.row_position(key).and_then(|at| self.rows.get(at))
    }

    fn row_position(&self, key: &RowKey) -> Option<usize> {
        let keying = keying(self.name.as_str());
        if self.rows.len() <= ROW_SCAN_LIMIT {
            return self.rows.iter().position(|r| r.has_key(keying, key));
        }
        self.rows.table(self.name.as_str()).keys.get(key).copied()
    }

    /// Puts `row` after the others, keeping the lookup table (if one is held) as it goes, so that a run of writes that each make a row is
    /// not a run of rebuilds.
    fn push_row(&mut self, row: Row) {
        self.rows.push_keeping_table(row, self.name.as_str());
    }
}

/// Which section a cell is in: its name and, for `Geometry`, its `IX`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SectionKey {
    pub name: SectionName,
    pub ix: Option<u32>,
}

impl SectionKey {
    /// A key for the section `name`: give an `ix` for `Geometry`.
    pub fn new(name: &str, ix: Option<u32>) -> SectionKey {
        SectionKey { name: SectionName::new(name), ix }
    }

    /// Whether `section` is the one this key names: the same name and the same `IX`, exactly. A key with no `IX` names a section with
    /// none (every section but `Geometry`); it does not name the first `Geometry` section, which would merge a local `Geometry` 3
    /// with the master's `Geometry` 0.
    fn matches(&self, section: &Section) -> bool {
        section.name == self.name && section.ix == self.ix
    }
}

/// Where a cell is in a sheet: directly under it, directly under a section, or in a row of a section.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellAddr {
    pub section: Option<SectionKey>,
    pub row: Option<RowKey>,
    pub cell: CellName,
}

impl CellAddr {
    /// A cell directly under the sheet: `PinX`.
    pub fn direct(cell: &str) -> CellAddr {
        CellAddr { section: None, row: None, cell: CellName::new(cell) }
    }

    /// A cell directly under a section: `NoFill` of a `Geometry` section.
    pub fn in_section(section: &str, ix: Option<u32>, cell: &str) -> CellAddr {
        CellAddr { section: Some(SectionKey::new(section, ix)), row: None, cell: CellName::new(cell) }
    }

    /// A cell in a row of a section: the `X` of `Geometry` row 2.
    pub fn in_row(section: &str, ix: Option<u32>, row: RowKey, cell: &str) -> CellAddr {
        CellAddr { section: Some(SectionKey::new(section, ix)), row: Some(row), cell: CellName::new(cell) }
    }
}

/// What a sheet holds at an address.
#[derive(Clone, Copy, Debug)]
pub enum Lookup<'a> {
    /// No such cell here: it may be inherited.
    Missing,
    /// The section or row is deleted here (`Del`): nothing in it is inherited.
    Deleted,
    Found(&'a Cell),
}

/// A sheet: the cells, sections and style links of a page, master, shape or style, and of the document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub links: Links,
    /// Cells directly under the sheet, by exact name.
    pub cells: Cells,
    pub sections: Vec<Arc<Section>>,
    /// Attributes of the element the model has no field for (a `PageSheet`'s `UniqueID`).
    pub raw: RawAttrs,
    /// Elements and other nodes the model has no field for: triggers, foreign-namespace children.
    pub passthrough: Passthrough,
    /// What the element holds that no field reads, tagged by format; see [`crate::package`].
    pub ext: Ext<Kept>,
}

impl Sheet {
    /// The cell directly under the sheet called `name`.
    pub fn cell(&self, name: &str) -> Option<&Cell> {
        self.cells.get(name)
    }

    /// The first section with this key.
    pub fn section(&self, key: &SectionKey) -> Option<&Arc<Section>> {
        self.sections.iter().inspect(|_| crate::work::section_examined()).find(|s| key.matches(s))
    }

    /// What this sheet itself holds at `addr`, which is nothing of what it inherits.
    pub fn lookup(&self, addr: &CellAddr) -> Lookup<'_> {
        self.lookup_with(addr, Sheet::section, Section::row)
    }

    /// [`Sheet::lookup`], finding a section of a sheet by `find_section` and a row of a section by `find_row`: a caller with an index of
    /// a sheet that has many sections, or of a large section, passes its own.
    pub(crate) fn lookup_with<'a>(
        &'a self,
        addr: &CellAddr,
        find_section: impl Fn(&'a Sheet, &SectionKey) -> Option<&'a Arc<Section>>,
        find_row: impl Fn(&'a Section, &RowKey) -> Option<&'a Arc<Row>>,
    ) -> Lookup<'a> {
        let Some(key) = &addr.section else {
            return found(&self.cells, &addr.cell);
        };
        let Some(section) = find_section(self, key) else {
            return Lookup::Missing;
        };
        if section.del {
            return Lookup::Deleted;
        }
        let Some(row_key) = &addr.row else {
            return found(&section.cells, &addr.cell);
        };
        match find_row(section, row_key) {
            None => Lookup::Missing,
            Some(row) if row.del => Lookup::Deleted,
            Some(row) => found(&row.cells, &addr.cell),
        }
    }

    /// `"section"` or `"row"` when the address is inside a section or row that is deleted here (`Del`).
    pub(crate) fn deleted_part(&self, addr: &CellAddr) -> Option<&'static str> {
        let section = self.section(addr.section.as_ref()?)?;
        if section.del {
            return Some("section");
        }
        section.row(addr.row.as_ref()?).filter(|row| row.del).map(|_| "row")
    }

    /// Checks the sheet against the limits a write keeps ([`MAX_SECTIONS_PER_SHEET`], [`MAX_ROWS_PER_SECTION`],
    /// [`MAX_CELLS_PER_CONTAINER`], [`MAX_NAME_BYTES`]): the check a reader of a file runs on each sheet it builds, since a document built
    /// by hand is not held to them. It costs a pass over the sheet's own cells, sections and rows.
    pub fn check_limits(&self) -> Result<(), DocError> {
        check_cells(&self.cells)?;
        if self.sections.len() > MAX_SECTIONS_PER_SHEET {
            return Err(DocError::Limit { what: "sections in a sheet", max: MAX_SECTIONS_PER_SHEET });
        }
        for section in &self.sections {
            check_name(section.name.as_str(), "a section name")?;
            check_cells(&section.cells)?;
            if section.rows.len() > MAX_ROWS_PER_SECTION {
                return Err(DocError::Limit { what: "rows in a section", max: MAX_ROWS_PER_SECTION });
            }
            for row in &section.rows {
                check_cells(&row.cells)?;
                if let Some(name) = &row.name {
                    check_name(name, "a row name")?;
                }
            }
        }
        Ok(())
    }

    /// Writes `cell` at `addr`, adding the section and the row if the sheet has neither, and returns
    /// the cell it replaced. Refuses an empty or enormous name, a section, row or cell past the limits, and an address in a deleted
    /// row or section ([`DocError::Deleted`]), and then changes nothing. Only what is on the way to the cell is copied: see the module documentation.
    pub fn set_cell(&mut self, addr: &CellAddr, cell: Cell) -> Result<Option<Cell>, DocError> {
        check_name(addr.cell.as_str(), "a cell name")?;
        let Some(key) = &addr.section else {
            return put(&mut self.cells, &addr.cell, cell);
        };
        check_name(key.name.as_str(), "a section name")?;
        if let Some(RowKey::Name(n)) = &addr.row {
            check_name(n, "a row name")?;
        }
        // A deleted row or section reads as gone, so a cell written into one would be lost: that is an error and
        // changes nothing. `Sheet::row_mut` reaches a deleted row, which is how it is brought back.
        if let Some(what) = self.deleted_part(addr) {
            return Err(DocError::Deleted(what));
        }
        let at = self.section_at_or_new(key)?;
        let section = self.sections.get_mut(at).ok_or(DocError::NoSuchSheet)?;
        let Some(row_key) = &addr.row else {
            // Checked before the copy: a write that is refused copies nothing.
            check_room(&section.cells, &addr.cell)?;
            return put(&mut Arc::make_mut(section).cells, &addr.cell, cell);
        };
        check_room_for_row(section, section.row_position(row_key), &addr.cell)?;
        put(&mut self.row_mut(key, row_key)?.cells, &addr.cell, cell)
    }

    /// The row with this key of this section, to change, making the section and the row if the sheet has neither. Only what is
    /// on the way to the row is copied. Refuses a name that is empty or enormous and a section or row past the limits, and then
    /// changes nothing.
    pub fn row_mut(&mut self, key: &SectionKey, row: &RowKey) -> Result<&mut Row, DocError> {
        check_name(key.name.as_str(), "a section name")?;
        if let RowKey::Name(n) = row {
            check_name(n, "a row name")?;
        }
        let at = self.section_at_or_new(key)?;
        let section = self.sections.get_mut(at).ok_or(DocError::NoSuchSheet)?;
        let existing = section.row_position(row);
        if existing.is_none() && section.rows.len() >= MAX_ROWS_PER_SECTION {
            return Err(DocError::Limit { what: "rows in a section", max: MAX_ROWS_PER_SECTION });
        }
        let section = Arc::make_mut(section);
        let row_at = match existing {
            Some(at) => at,
            None => {
                section.push_row(new_row(row));
                section.rows.len().saturating_sub(1)
            }
        };
        section.rows.slot_mut(row_at).map(Arc::make_mut).ok_or(DocError::NoSuchSheet)
    }

    /// The section with this key, to change (to mark it deleted, to give it cells of its own), making it if the sheet has none. Only what
    /// is on the way to it is copied. Refuses a name that is empty or enormous and a section past the limit, and then changes nothing.
    pub fn section_mut(&mut self, key: &SectionKey) -> Result<&mut Section, DocError> {
        check_name(key.name.as_str(), "a section name")?;
        let at = self.section_at_or_new(key)?;
        self.sections.get_mut(at).map(Arc::make_mut).ok_or(DocError::NoSuchSheet)
    }

    /// Where the section with this key is in the list, making it if it is not there.
    fn section_at_or_new(&mut self, key: &SectionKey) -> Result<usize, DocError> {
        if let Some(at) = self.sections.iter().position(|s| key.matches(s)) {
            return Ok(at);
        }
        if self.sections.len() >= MAX_SECTIONS_PER_SHEET {
            return Err(DocError::Limit { what: "sections in a sheet", max: MAX_SECTIONS_PER_SHEET });
        }
        self.sections.push(Arc::new(Section::new(key.name.as_str(), key.ix)));
        Ok(self.sections.len().saturating_sub(1))
    }

    /// Takes out the cell at `addr`, returning it. An emptied row or section is left, as the file would hold it.
    pub fn remove_cell(&mut self, addr: &CellAddr) -> Option<Cell> {
        if !matches!(self.lookup(addr), Lookup::Found(_)) {
            return None;
        }
        let Some(key) = &addr.section else {
            return self.cells.remove(addr.cell.as_str());
        };
        let at = self.sections.iter().position(|s| key.matches(s))?;
        let section = Arc::make_mut(self.sections.get_mut(at)?);
        let Some(row_key) = &addr.row else {
            return section.cells.remove(addr.cell.as_str());
        };
        let row_at = section.row_position(row_key)?;
        Arc::make_mut(section.rows.slot_mut(row_at)?).cells.remove(addr.cell.as_str())
    }
}

fn found<'a>(cells: &'a Cells, name: &CellName) -> Lookup<'a> {
    cells.get(name.as_str()).map_or(Lookup::Missing, Lookup::Found)
}

/// The names and the count of one place's cells against the limits.
fn check_cells(cells: &Cells) -> Result<(), DocError> {
    if cells.len() > MAX_CELLS_PER_CONTAINER {
        return Err(DocError::Limit { what: "cells in one place", max: MAX_CELLS_PER_CONTAINER });
    }
    cells.keys().try_for_each(|name| check_name(name.as_str(), "a cell name"))
}

fn check_name(name: &str, what: &'static str) -> Result<(), DocError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES { Err(DocError::BadName(what)) } else { Ok(()) }
}

fn new_row(key: &RowKey) -> Row {
    match key {
        RowKey::Ix(ix) => Row { ix: Some(*ix), ..Row::default() },
        RowKey::Name(n) => Row { name: Some(Arc::clone(n)), ..Row::default() },
    }
}

/// Whether `cells` can take one more cell called `name`: replacing a cell needs no room.
fn check_room(cells: &Cells, name: &CellName) -> Result<(), DocError> {
    if cells.len() >= MAX_CELLS_PER_CONTAINER && !cells.contains_key(name) {
        return Err(DocError::Limit { what: "cells in one place", max: MAX_CELLS_PER_CONTAINER });
    }
    Ok(())
}

fn check_room_for_row(section: &Arc<Section>, existing: Option<usize>, name: &CellName) -> Result<(), DocError> {
    match existing.and_then(|at| section.rows.get(at)) {
        Some(row) => check_room(&row.cells, name),
        None if section.rows.len() >= MAX_ROWS_PER_SECTION => Err(DocError::Limit { what: "rows in a section", max: MAX_ROWS_PER_SECTION }),
        None => Ok(()),
    }
}

fn put(cells: &mut Cells, name: &CellName, cell: Cell) -> Result<Option<Cell>, DocError> {
    check_room(cells, name)?;
    Ok(cells.insert(name.clone(), cell))
}
