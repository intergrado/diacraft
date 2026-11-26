//! The outline of a sheet (design 6): the sections it shows in the window's order, and for each the keys of its rows and the number of its lines, found by
//! one walk of the sheet's sections and rows and of the sheets it inherits from, with **no cell worked out or resolved**.
//!
//! **Lines.** A section is laid out in lines, as the window paints it: a **header** line, for a Geometry section its five own cells as **cell** lines, a line of column
//! **titles** (a section of rows), one line a **row**, and when a section is too long to list a last **note** line saying how many rows are not listed. A display group is a header
//! line and one cell line a cell. The lines of the sheet are numbered from 0 through its sections in order, and [`line_at`] finds the section of a line by a binary search of the
//! sections' first lines.
//!
//! **Budgets** (design 6). At most [`MAX_OUTLINE_LINES`](super::MAX_OUTLINE_LINES) lines are listed: a section past that lists the rows that fit and a note line, and a section
//! after the budget is spent lists none and says how many it holds (counted without a merge). The row keys held are those of the lines, 24 bytes each at most, so 24 MiB at the
//! most. The outline costs the sections and rows of the sheet and of each sheet it inherits from, once ([`Counts`]): the model's own walk (`Resolver::effective_row_keys`)
//! visits each row of each layer once.
//!
//! **Which sections** are those of `show.rs` and **in the order** of (`DISPLAY_ORDER` of `diacraft-doc`, the window's own order, of the
//! window's spec): the sections of the table in the table's order, a Geometry section by `IX`, then any section the sheet holds that the table does not list, by name, then **Other
//! Cells**: the cells directly under the sheet that no display group lists.

use std::collections::HashSet;

use diacraft_doc::{DISPLAY_ORDER, DisplaySection, Resolver, RowKey, SectionKey, SectionKind, SheetKind, SheetRef, display_group, section_spelling};

use super::MAX_OUTLINE_LINES;
use super::show::{Facts, group_shown};

/// The work an outline or a read of lines did, as counts: a test holds a cost to these, never to a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Sections looked at.
    pub sections: u64,
    /// Rows merged: one for each row of each section the outline lists, the rows of a section being merged once.
    pub rows: u64,
    /// Cells looked up through the resolver to tell which sections show (at most 18 for a shape, none for another sheet).
    pub probes: u64,
    /// Cells resolved to be shown: the cells of the lines read.
    pub cells: u64,
}

/// What an outline is asked for.
#[derive(Clone, Debug)]
pub struct Options {
    /// Also the groups leaves out for this kind of sheet.
    pub all: bool,
    /// Only the sections with these names (the `section` or the `label` of an answer, in any letter case, spaces ignored).
    pub only: Option<Vec<String>>,
    /// The most lines the outline lists: [`MAX_OUTLINE_LINES`] unless a test asks for fewer.
    pub max_lines: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options { all: false, only: None, max_lines: MAX_OUTLINE_LINES }
    }
}

/// Why an outline could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewError {
    /// The sheet is not in the document.
    NoSheet,
    /// A section asked for is not shown: `hidden` when leaves it out of this sheet and `all` would show it.
    NoSection { name: String, hidden: bool, shown: Vec<String> },
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewError::NoSheet => f.write_str("the sheet is not in the drawing"),
            ViewError::NoSection { name, hidden: true, .. } => {
                write!(f, "{name} is not shown on this sheet: the rules of the window leave it out; give all: true to read it")
            }
            ViewError::NoSection { name, hidden: false, shown } => {
                let list: Vec<&str> = shown.iter().take(16).map(String::as_str).collect();
                write!(
                    f,
                    "this sheet shows no section {name}: it shows {}{}",
                    list.join(", "),
                    if shown.len() > list.len() { ", and more" } else { "" }
                )
            }
        }
    }
}

impl std::error::Error for ViewError {}

/// What is at a place of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Header,
    /// A cell of the section itself (Geometry's five), the nth.
    Flag(usize),
    /// The nth cell of a display group.
    Cell(usize),
    Titles,
    /// The nth listed row.
    Row(usize),
    /// How many rows are not listed.
    Note,
}

/// One section of the outline.
#[derive(Clone, Debug)]
pub struct SectionInfo {
    /// What an answer calls the section: the spelling a reference uses (`Geometry2`, `Prop`, `User`) for a section of rows, the label for a display group.
    pub name: String,
    /// What the window titles it (`Geometry 2`, `Line Gradient Stops`, `Shape Data`).
    pub label: String,
    /// A Geometry section's `IX`.
    pub ix: Option<u32>,
    /// The table's entry, none for a section the table does not list.
    pub display: Option<&'static DisplaySection>,
    /// The section of rows: where its rows are looked up. `None` for a display group.
    pub key: Option<SectionKey>,
    /// The cells of a display group, or of Other Cells: their names, one line each.
    pub cells: Vec<String>,
    /// The rows listed, in order.
    pub rows: Vec<RowKey>,
    /// The rows the section holds, listed or not.
    pub rows_total: usize,
    /// The rows not listed (the section is past the budget).
    pub unlisted: usize,
    /// The titles of the columns of a section of rows: the cells the table lists for it (after the section's own), then any other cell a row holds.
    pub columns: Vec<String>,
    /// The cells of the section itself that come before its columns: five for Geometry.
    pub flags: Vec<&'static str>,
    pub first_line: u64,
    pub lines: u64,
    pub can_add_rows: bool,
    pub can_delete: bool,
}

impl SectionInfo {
    pub fn kind(&self) -> SectionKind {
        if self.key.is_some() { SectionKind::Rows } else { SectionKind::Cells }
    }

    /// What is at line `offset` of the section, or `None` past it.
    pub fn at(&self, offset: u64) -> Option<Slot> {
        if offset >= self.lines {
            return None;
        }
        let at = usize::try_from(offset).ok()?;
        if at == 0 {
            return Some(Slot::Header);
        }
        if self.key.is_none() {
            return Some(Slot::Cell(at - 1));
        }
        let flags = self.flags.len();
        if at <= flags {
            return Some(Slot::Flag(at - 1));
        }
        if at == flags + 1 {
            return Some(Slot::Titles);
        }
        let row = at - flags - 2;
        Some(if row < self.rows.len() { Slot::Row(row) } else { Slot::Note })
    }
}

/// The outline of a sheet.
#[derive(Clone, Debug)]
pub struct Outline {
    pub sections: Vec<SectionInfo>,
    /// The first line of each section: strictly rising.
    pub firsts: Vec<u64>,
    pub total_lines: u64,
    pub counts: Counts,
}

/// The section that holds `line` and its place in the section, by a binary search of the sections' first lines. `firsts` rise strictly, each section holds at least its header,
/// and `total` is the number of lines of all. `None` past the last line.
pub fn line_at(firsts: &[u64], total: u64, line: u64) -> Option<(usize, u64)> {
    if line >= total {
        return None;
    }
    let at = firsts.partition_point(|first| *first <= line).checked_sub(1)?;
    let first = *firsts.get(at)?;
    Some((at, line.checked_sub(first)?))
}

/// What the window titles a section of rows: Geometry by its number, the two gradients as the window's images spell them.
pub(crate) fn label_of(display: Option<&DisplaySection>, key: &SectionKey) -> String {
    let element = key.name.as_str();
    if element == "Geometry" {
        return match key.ix.and_then(|ix| ix.checked_add(1)) {
            Some(n) => format!("Geometry {n}"),
            None => "Geometry".to_owned(),
        };
    }
    match element {
        "LineGradient" => "Line Gradient Stops".to_owned(),
        "FillGradient" => "Fill Gradient Stops".to_owned(),
        _ => display.map_or_else(|| element.to_owned(), |d| d.label.to_owned()),
    }
}

/// What an answer calls a section of rows: the spelling a reference uses.
pub(crate) fn name_of(display: Option<&DisplaySection>, key: &SectionKey) -> String {
    let element = key.name.as_str();
    if element == "Geometry" {
        return match key.ix.and_then(|ix| ix.checked_add(1)) {
            Some(n) => format!("Geometry{n}"),
            None => "Geometry".to_owned(),
        };
    }
    section_spelling(element).map_or_else(|| display.map_or_else(|| element.to_owned(), |d| d.label.to_owned()), str::to_owned)
}

/// A name as it is compared when a section is asked for: no spaces, in lower case.
fn norm(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}

/// What the walk of the sections of one sheet works with.
#[derive(Clone, Copy)]
struct Scope<'r, 'd> {
    res: &'r Resolver<'d>,
    sheet: &'r SheetRef,
    kind: SheetKind,
    max_lines: u64,
}

/// What an outline plans for a section before it looks at its rows.
enum Planned {
    Group(&'static DisplaySection),
    Rows { display: Option<&'static DisplaySection>, key: SectionKey },
    Other(Vec<String>),
}

impl Planned {
    fn names(&self) -> (String, String) {
        match self {
            Planned::Group(d) => (d.label.to_owned(), d.label.to_owned()),
            Planned::Rows { display, key } => (name_of(*display, key), label_of(*display, key)),
            Planned::Other(_) => ("Other Cells".to_owned(), "Other Cells".to_owned()),
        }
    }
}

/// The most columns a section of rows gets beyond the cells the table lists for it.
const MAX_EXTRA_COLUMNS: usize = 256;

impl Outline {
    /// The outline of `sheet` in the document `res` reads. `Err` when the sheet is not there, or a section asked for is not shown.
    pub fn build(res: &Resolver<'_>, sheet: &SheetRef, options: &Options) -> Result<Outline, ViewError> {
        let local = res.sheet(sheet).ok_or(ViewError::NoSheet)?;
        let kind = SheetKind::of(sheet);
        let mut counts = Counts::default();
        let facts = Facts::of(res, sheet, local, &mut counts);

        // The sections of rows the sheet holds, by the table's entry that names them; a section the table does not list, or one named like a display group, is an extension.
        let names = res.effective_section_names(sheet).ok_or(ViewError::NoSheet)?;
        let mut by_entry: Vec<Vec<SectionKey>> = vec![Vec::new(); DISPLAY_ORDER.len()];
        let mut extension: Vec<SectionKey> = Vec::new();
        for name in &names {
            let keys = res.effective_sections(sheet, name.as_str()).unwrap_or_default();
            let entry =
                DISPLAY_ORDER.iter().position(|d| d.kind == SectionKind::Rows && (d.label == name.as_str() || d.element == Some(name.as_str())));
            match entry.and_then(|i| by_entry.get_mut(i)) {
                Some(list) => list.extend(keys),
                None => extension.extend(keys),
            }
        }
        extension.sort_by(|a, b| a.name.cmp(&b.name).then(a.ix.cmp(&b.ix)));

        let mut planned: Vec<Planned> = Vec::new();
        for (d, keys) in DISPLAY_ORDER.iter().zip(by_entry) {
            match d.kind {
                SectionKind::Cells => {
                    if group_shown(d.label, &facts, options.all) {
                        planned.push(Planned::Group(d));
                    }
                }
                SectionKind::Rows => planned.extend(keys.into_iter().map(|key| Planned::Rows { display: Some(d), key })),
            }
        }
        planned.extend(extension.into_iter().map(|key| Planned::Rows { display: None, key }));
        let others: Vec<String> = local.cells.keys().filter(|n| display_group(n.as_str()).is_none()).map(|n| n.as_str().to_owned()).collect();
        if !others.is_empty() {
            planned.push(Planned::Other(others));
        }
        counts.sections = counts.sections.saturating_add(planned.len() as u64);

        if let Some(only) = &options.only {
            let wanted: HashSet<String> = only.iter().map(|n| norm(n)).collect();
            let named: Vec<(String, String)> = planned.iter().map(Planned::names).collect();
            // Each section's two names, normalised once.
            let normed: Vec<(String, String)> = named.iter().map(|(name, label)| (norm(name), norm(label))).collect();
            let found: HashSet<&str> = normed.iter().flat_map(|(name, label)| [name.as_str(), label.as_str()]).collect();
            for given in only {
                let w = norm(given);
                if !found.contains(w.as_str()) {
                    let hidden =
                        DISPLAY_ORDER.iter().any(|d| d.kind == SectionKind::Cells && norm(d.label) == w && group_shown(d.label, &facts, true));
                    return Err(ViewError::NoSection { name: given.clone(), hidden, shown: named.into_iter().map(|(_, label)| label).collect() });
                }
            }
            let mut keep = normed.iter().map(|(name, label)| wanted.contains(name) || wanted.contains(label));
            planned.retain(|_| keep.next().unwrap_or(false));
        }

        let scope = Scope { res, sheet, kind, max_lines: options.max_lines };
        let mut sections: Vec<SectionInfo> = Vec::new();
        let mut next_line = 0_u64;
        for plan in planned {
            let info = match plan {
                Planned::Group(d) => group_info(d, &facts, kind, next_line),
                Planned::Other(cells) => other_info(cells, next_line),
                Planned::Rows { display, key } => match rows_info(&scope, display, key, next_line, &mut counts) {
                    Some(info) => info,
                    None => continue,
                },
            };
            next_line = next_line.saturating_add(info.lines);
            sections.push(info);
        }
        let firsts = sections.iter().map(|s| s.first_line).collect();
        Ok(Outline { sections, firsts, total_lines: next_line, counts })
    }

    /// The section that holds `line` and the place in it.
    pub fn at(&self, line: u64) -> Option<(usize, u64)> {
        line_at(&self.firsts, self.total_lines, line)
    }

    /// The section with this name or label.
    pub fn section(&self, name: &str) -> Option<&SectionInfo> {
        let n = norm(name);
        self.sections.iter().find(|s| norm(&s.name) == n || norm(&s.label) == n)
    }
}

fn group_info(d: &'static DisplaySection, facts: &Facts, kind: SheetKind, first_line: u64) -> SectionInfo {
    let cells: Vec<String> = d.cells.iter().map(|c| (*c).to_owned()).collect();
    // Text Transform and Layer Membership are inserted and deleted whole: a group that is shown only because `all` asked has nothing to delete.
    let held = match d.label {
        "Text Transform" => facts.text_transform,
        "Layer Membership" => facts.layer_member,
        _ => true,
    };
    SectionInfo {
        name: d.label.to_owned(),
        label: d.label.to_owned(),
        ix: None,
        display: Some(d),
        key: None,
        lines: 1 + cells.len() as u64,
        cells,
        rows: Vec::new(),
        rows_total: 0,
        unlisted: 0,
        columns: Vec::new(),
        flags: Vec::new(),
        first_line,
        can_add_rows: false,
        can_delete: held && d.rules.can_delete_on(kind),
    }
}

fn other_info(cells: Vec<String>, first_line: u64) -> SectionInfo {
    SectionInfo {
        name: "Other Cells".to_owned(),
        label: "Other Cells".to_owned(),
        ix: None,
        display: None,
        key: None,
        lines: 1 + cells.len() as u64,
        cells,
        rows: Vec::new(),
        rows_total: 0,
        unlisted: 0,
        columns: Vec::new(),
        flags: Vec::new(),
        first_line,
        can_add_rows: false,
        can_delete: false,
    }
}

/// A section of rows, or `None` when the sheet holds none of its rows and none of its own cells (every row is deleted).
fn rows_info(
    scope: &Scope<'_, '_>,
    display: Option<&'static DisplaySection>,
    key: SectionKey,
    first_line: u64,
    counts: &mut Counts,
) -> Option<SectionInfo> {
    let Scope { res, sheet, kind, max_lines } = *scope;
    let geometry = key.name.as_str() == "Geometry";
    // Geometry's five own cells come first in the table's list; the rest are the columns.
    let flag_count = if geometry { 5 } else { 0 };
    let listed_cells: &[&'static str] = display.map_or(&[], |d| d.cells);
    let flags: Vec<&'static str> = listed_cells.iter().take(flag_count).copied().collect();
    let overhead = 2 + flags.len() as u64;

    let (rows, rows_total, unlisted, extra) = if first_line.saturating_add(overhead) > max_lines {
        // The budget is spent: the section is named and counted, with no merge.
        let held = res.rows_held(sheet, &key).unwrap_or(0);
        (Vec::new(), held, held, Vec::new())
    } else {
        let merged = res.effective_row_keys(sheet, &key).unwrap_or_default();
        counts.rows = counts.rows.saturating_add(merged.len() as u64);
        let columns: HashSet<&str> = listed_cells.iter().skip(flag_count).copied().collect();
        let mut extra: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for row in &merged {
            for cell in &row.cells {
                let name = cell.as_str();
                if !columns.contains(name) && extra.len() < MAX_EXTRA_COLUMNS && !seen.contains(name) {
                    seen.insert(name.to_owned());
                    extra.push(name.to_owned());
                }
            }
        }
        extra.sort();
        let total = merged.len();
        let room = usize::try_from(max_lines.saturating_sub(first_line).saturating_sub(overhead)).unwrap_or(usize::MAX);
        // A section too long for the budget keeps a line for its note.
        let listed = if total <= room { total } else { room.saturating_sub(1) };
        let keys: Vec<RowKey> = merged.into_iter().take(listed).map(|row| row.key).collect();
        (keys, total, total - listed, extra)
    };
    if rows_total == 0 && !has_own_cells(res, sheet, &key) {
        return None;
    }
    let mut columns: Vec<String> = listed_cells.iter().skip(flag_count).map(|c| (*c).to_owned()).collect();
    columns.extend(extra);
    let rules = display.map(|d| d.rules);
    let lines = overhead.saturating_add(rows.len() as u64).saturating_add(u64::from(unlisted > 0));
    Some(SectionInfo {
        name: name_of(display, &key),
        label: label_of(display, &key),
        ix: key.ix,
        display,
        cells: Vec::new(),
        rows,
        rows_total,
        unlisted,
        columns,
        flags,
        first_line,
        lines,
        can_add_rows: rules.is_some_and(|r| r.rows_addable),
        can_delete: rules.is_some_and(|r| r.can_delete_on(kind)),
        key: Some(key),
    })
}

/// Whether some sheet the section is looked for on holds a cell of the section itself (Geometry's `NoFill`).
fn has_own_cells(res: &Resolver<'_>, sheet: &SheetRef, key: &SectionKey) -> bool {
    res.section_layers(sheet, key).is_some_and(|layers| layers.iter().any(|s| !s.cells.is_empty()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn a_line_belongs_to_the_last_section_that_begins_at_or_before_it() {
        let firsts = [0, 3, 4, 10];
        assert_eq!(line_at(&firsts, 12, 0), Some((0, 0)));
        assert_eq!(line_at(&firsts, 12, 2), Some((0, 2)));
        assert_eq!(line_at(&firsts, 12, 3), Some((1, 0)));
        assert_eq!(line_at(&firsts, 12, 4), Some((2, 0)));
        assert_eq!(line_at(&firsts, 12, 9), Some((2, 5)));
        assert_eq!(line_at(&firsts, 12, 11), Some((3, 1)));
        assert_eq!(line_at(&firsts, 12, 12), None, "past the last line");
        assert_eq!(line_at(&[], 0, 0), None);
        assert_eq!(line_at(&[], 5, 0), None, "no section to hold it");
        assert_eq!(line_at(&firsts, u64::MAX, u64::MAX - 1), Some((3, u64::MAX - 11)));
    }

    #[test]
    fn the_places_of_a_section_are_its_header_its_flags_its_titles_its_rows_and_its_note() {
        let mut section = SectionInfo {
            name: "Geometry1".into(),
            label: "Geometry 1".into(),
            ix: Some(0),
            display: None,
            key: Some(SectionKey::new("Geometry", Some(0))),
            cells: Vec::new(),
            rows: vec![RowKey::Ix(1), RowKey::Ix(2)],
            rows_total: 5,
            unlisted: 3,
            columns: Vec::new(),
            flags: vec!["NoFill", "NoLine"],
            first_line: 0,
            lines: 2 + 2 + 2 + 1,
            can_add_rows: true,
            can_delete: true,
        };
        let slots: Vec<Option<Slot>> = (0..8).map(|o| section.at(o)).collect();
        assert_eq!(
            slots,
            [
                Some(Slot::Header),
                Some(Slot::Flag(0)),
                Some(Slot::Flag(1)),
                Some(Slot::Titles),
                Some(Slot::Row(0)),
                Some(Slot::Row(1)),
                Some(Slot::Note),
                None
            ]
        );
        section.key = None;
        section.cells = vec!["A".into(), "B".into()];
        section.lines = 3;
        assert_eq!(
            (section.at(0), section.at(1), section.at(2), section.at(3)),
            (Some(Slot::Header), Some(Slot::Cell(0)), Some(Slot::Cell(1)), None)
        );
    }

    #[test]
    fn a_name_matches_without_its_spaces_or_its_case() {
        assert_eq!(norm("Geometry 1"), norm("geometry1"));
        assert_ne!(norm("Geometry 1"), norm("Geometry 2"));
    }
}
