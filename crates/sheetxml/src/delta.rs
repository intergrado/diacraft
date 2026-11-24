//! What a writer that writes only what differs needs: the cells, sections and rows of a sheet that a reader would not rebuild by itself.
//!
//! A format that has its own native way to say most of a shape (a drawing in `.odg` says position, size, colours and text in OpenDocument's
//! own markup) writes the cells that markup cannot say, and only those: a reader rebuilds the rest from the native markup, and the cells it
//! rebuilds are its **defaults**. [`cells_delta`] is the comparison for the cells of one container: given the cells the model holds and the
//! cells the reader would rebuild, which cells are to be written, and which of the reader's own are gone and are to be said gone.
//! [`sheet_delta`] is the same for a whole sheet: its own cells, and each section and each row of a section, compared by key
//! ([`SectionKey`], [`RowKey`]) and by value.
//!
//! **What is compared, and how.** Everything the model keeps of a cell, a row or a section: a cell's formula, value, unit, error, attributes
//! and content; a row's name, index, local name, type, `Del` flag, attributes and unknown nodes; a section's index, flag, attributes and unknown
//! nodes. Two things are the same when the model says they are equal, so a caller that wants "equal within a tolerance" or "equal but for the
//! unit" gives the comparison the defaults it has made equal in those respects.

use std::collections::{HashMap, HashSet};

use diacraft_doc::{Cell, CellName, Cells, Row, RowKey, Section, SectionKey, Sheet};

/// The cells of a container that differ from what a reader would rebuild.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellDelta<'a> {
    /// The cells to write: a cell the reader would not rebuild, or would rebuild otherwise (another formula, value, unit, error, content or
    /// attribute), in the order of their names.
    pub changed: Vec<(&'a CellName, &'a Cell)>,
    /// The names of cells the reader would rebuild and the model no longer has: they are to be said gone. (A name is a reference count or a
    /// static text, so these are copies, and the defaults need not outlive the delta: a caller builds them to compare and drops them.)
    pub removed: Vec<CellName>,
}

impl CellDelta<'_> {
    /// Whether the model holds exactly the cells the reader would rebuild: nothing is to be written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

/// The cells of `now` that differ from `defaults`, and the names of the cells of `defaults` that `now` has not. Cells are compared as the model
/// compares them (every field, `raw` and content included), so a cell is written whenever anything of it would not come back.
///
/// The cost is one lookup for each cell of either, so it is bounded by the model's cap on the cells of one place.
#[must_use]
pub fn cells_delta<'a>(now: &'a Cells, defaults: &Cells) -> CellDelta<'a> {
    let changed = now.iter().filter(|(name, cell)| defaults.get(*name) != Some(*cell)).collect();
    let removed = defaults.keys().filter(|name| !now.contains_key(*name)).cloned().collect();
    CellDelta { changed, removed }
}

/// Whether a cell can be written as attributes alone: it has no text and no child of its own (`Cell::extra`). A cell that has any cannot be
/// written as the attributes of [`crate::build::cell_attrs`] without losing what is inside it.
#[must_use]
pub fn fits_attributes(cell: &Cell) -> bool {
    cell.extra.is_none()
}

/// A section the reader would not rebuild as it is.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionDelta<'a> {
    /// The model's section.
    pub section: &'a Section,
    /// The reader would rebuild no section of this key: the section is written whole.
    pub new: bool,
    /// The section's own cells (a `Geometry` section's flags) that differ, and those the reader would rebuild that the model lacks.
    pub cells: CellDelta<'a>,
    /// The section's own index, flag, attributes or unknown nodes differ from the reader's.
    pub attributes: bool,
    /// The rows the reader would not rebuild as they are, in the section's order: a row of a key the reader has no row of, or one that differs
    /// in anything it keeps. A row with no key is always among them (a key is how a reader finds a row again).
    pub rows: Vec<&'a Row>,
    /// The keys of rows the reader would rebuild and the model lacks.
    pub removed_rows: Vec<RowKey>,
    /// The rows both hold are in another order in the model: the section is better written whole than row by row.
    pub reordered: bool,
}

impl SectionDelta<'_> {
    /// Whether nothing of the section is to be written: the reader rebuilds it as it is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.new && self.cells.is_empty() && !self.attributes && self.rows.is_empty() && self.removed_rows.is_empty() && !self.reordered
    }
}

/// What a sheet holds that a reader would not rebuild.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetDelta<'a> {
    /// The sheet's own cells.
    pub cells: CellDelta<'a>,
    /// The sections that are new or differ, in the model's order; one that is the same is not among them.
    pub sections: Vec<SectionDelta<'a>>,
    /// The keys of sections the reader would rebuild and the model lacks.
    pub removed_sections: Vec<SectionKey>,
}

impl SheetDelta<'_> {
    /// Whether the model holds exactly the cells, rows and sections the reader would rebuild.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.sections.is_empty() && self.removed_sections.is_empty()
    }
}

/// The key of a section, with how many sections of that key came before it: a sheet with two sections of one key (a file may have them) is
/// compared first with first and second with second.
fn section_slots(sections: &[std::sync::Arc<Section>]) -> Vec<(SectionKey, usize)> {
    let mut seen: HashMap<SectionKey, usize> = HashMap::new();
    sections
        .iter()
        .map(|s| {
            let key = SectionKey { name: s.name.clone(), ix: s.ix };
            let n = seen.entry(key.clone()).or_insert(0);
            let slot = (key, *n);
            *n += 1;
            slot
        })
        .collect()
}

/// The row of `section` for each key, the first of a key.
fn rows_by_key<'a>(section: &'a Section) -> HashMap<RowKey, &'a Row> {
    let mut by_key: HashMap<RowKey, &'a Row> = HashMap::new();
    for row in section.rows.iter() {
        if let Some(key) = row.key(section.name.as_str()) {
            by_key.entry(key).or_insert(row);
        }
    }
    by_key
}

/// How `now` stands against `rebuilt`, a section the reader has under the same key.
fn section_delta<'a>(now: &'a Section, rebuilt: &Section) -> SectionDelta<'a> {
    let name = now.name.as_str();
    let held = rows_by_key(rebuilt);
    let mut present: HashSet<RowKey> = HashSet::new();
    let mut rows: Vec<&'a Row> = Vec::new();
    let mut order_now: Vec<RowKey> = Vec::new();
    for row in now.rows.iter() {
        match row.key(name) {
            Some(key) => {
                present.insert(key.clone());
                if held.get(&key).copied() != Some(&**row) {
                    rows.push(&**row);
                }
                order_now.push(key);
            }
            None => rows.push(&**row),
        }
    }
    let removed_rows: Vec<RowKey> = rebuilt
        .rows
        .iter()
        .filter_map(|r| r.key(rebuilt.name.as_str()))
        .filter(|k| !present.contains(k))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    // The keys both hold, in the order each has them.
    let order_held: Vec<RowKey> = rebuilt.rows.iter().filter_map(|r| r.key(name)).filter(|k| present.contains(k)).collect();
    let shared_now: Vec<&RowKey> = order_now.iter().filter(|k| held.contains_key(*k)).collect();
    let reordered = shared_now.len() != order_held.len() || shared_now.iter().zip(&order_held).any(|(a, b)| *a != b);
    let mut removed_rows = removed_rows;
    removed_rows.sort();
    SectionDelta {
        section: now,
        new: false,
        cells: cells_delta(&now.cells, &rebuilt.cells),
        attributes: now.ix != rebuilt.ix || now.del != rebuilt.del || now.raw != rebuilt.raw || now.passthrough != rebuilt.passthrough,
        rows,
        removed_rows,
        reordered,
    }
}

/// What `now` holds that a reader would not rebuild from its own native markup, `defaults` being the sheet the reader would rebuild: the
/// cells, and for each section the model has and the reader would not rebuild as it is, what differs (a section the reader would not have at
/// all is [`SectionDelta::new`], to be written whole), and the sections the reader would rebuild that the model has not.
///
/// Sections are matched by [`SectionKey`] (and, for two of one key, by their order), rows by [`RowKey`]. The cost is a lookup for each cell,
/// row and section of either, so it is bounded by the model's caps.
#[must_use]
pub fn sheet_delta<'a>(now: &'a Sheet, defaults: &Sheet) -> SheetDelta<'a> {
    let cells = cells_delta(&now.cells, &defaults.cells);
    let held: HashMap<(SectionKey, usize), &Section> =
        section_slots(&defaults.sections).into_iter().zip(defaults.sections.iter()).map(|(slot, s)| (slot, &**s)).collect();
    let mut sections: Vec<SectionDelta<'a>> = Vec::new();
    let mut present: HashSet<(SectionKey, usize)> = HashSet::new();
    for (slot, section) in section_slots(&now.sections).into_iter().zip(now.sections.iter()) {
        present.insert(slot.clone());
        let delta = match held.get(&slot) {
            None => SectionDelta {
                section,
                new: true,
                cells: CellDelta::default(),
                attributes: false,
                rows: Vec::new(),
                removed_rows: Vec::new(),
                reordered: false,
            },
            Some(rebuilt) if **section == **rebuilt => continue,
            Some(rebuilt) => section_delta(section, rebuilt),
        };
        if !delta.is_empty() {
            sections.push(delta);
        }
    }
    let mut removed_sections: Vec<SectionKey> =
        section_slots(&defaults.sections).into_iter().filter(|slot| !present.contains(slot)).map(|(key, _)| key).collect();
    removed_sections.dedup();
    SheetDelta { cells, sections, removed_sections }
}
