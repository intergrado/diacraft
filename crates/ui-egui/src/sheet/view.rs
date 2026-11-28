//! What a tab keeps of the sheet it shows (design 6): the outline of its sections, and the cells of the lines last seen.
//!
//! * **The outline** is made once for each drawing generation, revision and object, by one walk of the sheet's sections and rows and of the sheets it inherits from, **with no cell
//!   resolved** (`sheetview::Outline::build`). The generation is in the key because a new `Session` starts its revision again: the lesson, a cache keyed by the
//!   revision alone shows the last file after an open. **Show Sections and a fold do not make it again**: the window arranges the outline it holds (a hidden section is left out,
//!   a folded one is a header), which is one pass over the sections.
//! * **The visual lines** (`pairs.rs`) are made from the arranged outline and the width of the list: a display group's cells share lines in columns of pairs. The window
//!   scrolls, selects and walks by visual lines, and reads the engine lines that the visual lines on view stand for.
//! * **The lines** on view are resolved once and kept (`View::read_visual`): a frame in which nothing changed resolves none, a scroll that brings k new lines on view resolves those k,
//!   and lines far from the view are let go, so that a sheet of a million lines holds a few hundred.
//! * **The selected cell** is found through the outline: where its line is, and its cell read for the bar.

use std::collections::{HashMap, HashSet};

use diacraft_engine::doc::{Document, Resolver, RowKey, SheetRef};
use diacraft_engine::sheetview::{CellView, Counts, Line, Options, Outline, Place, SectionInfo, Slot, read_lines};

use super::pairs::{EngineLines, VSlot, Visual};

/// The lines kept beyond the view at each end, so that a scroll of a few lines resolves none that were seen a moment ago.
pub const KEEP_LINES: u64 = 120;

/// The most lines held: past it, those far from the view go.
pub const MAX_CACHED: usize = 800;

/// A cell or a row of a sheet, by what names it and not by its place: a place changes when lines are folded or sections are hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    /// A cell: of a display group (no row), of a section's own cells (Geometry's five: a section of rows and no row), or of a row.
    Cell { section: String, row: Option<RowKey>, cell: String },
    /// A whole row of a section of rows.
    Row { section: String, row: RowKey },
    /// A section's header: the keys fold it (Space or Enter).
    Header { section: String },
}

impl Sel {
    pub fn section(&self) -> &str {
        match self {
            Sel::Cell { section, .. } | Sel::Row { section, .. } | Sel::Header { section } => section,
        }
    }
}

/// A place a key can stop at in a visual line: its column, what is selected, and whether the line holds its cells in columns that a move up or down keeps to (a line of pairs: a
/// column that has no cell in a line is passed over) or in any column the move can come to (a row of a table, a header).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub col: usize,
    pub sel: Sel,
    pub strict: bool,
}

/// The work a view did, as counts a test holds a cost to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub lines: u64,
    pub cells: u64,
}

/// A sheet as a tab shows it for one revision of one drawing.
pub struct View {
    /// The drawing's generation and revision it was made for.
    pub generation: u64,
    pub revision: u64,
    /// The sheet, or `None` where its object is not in the drawing any more.
    pub sheet: Option<SheetRef>,
    /// The rows the walk visited to make the outline: for a test's count.
    pub rows_visited: u64,
    base: Outline,
    arranged: Option<Outline>,
    arranged_for: Option<(u64, usize)>,
    visual: Visual,
    lines: HashMap<u64, Line>,
}

/// The outline of a sheet with some sections left out and some folded to their headers: the lines are numbered again, and nothing is looked up.
fn arranged(base: &Outline, hidden: &HashSet<String>, folded: &HashSet<String>) -> Outline {
    let mut sections: Vec<SectionInfo> = Vec::with_capacity(base.sections.len());
    let mut next = 0_u64;
    for section in base.sections.iter().filter(|s| !hidden.contains(&s.name)) {
        let mut section = section.clone();
        if folded.contains(&section.name) {
            section.lines = 1;
        }
        section.first_line = next;
        next = next.saturating_add(section.lines);
        sections.push(section);
    }
    let firsts = sections.iter().map(|s| s.first_line).collect();
    Outline { sections, firsts, total_lines: next, counts: base.counts }
}

impl View {
    /// The view of `place` in `doc`, made for `generation` and `revision`. A place whose object is gone gives a view with no sheet.
    pub fn build(doc: &Document, place: &Place, generation: u64, revision: u64) -> View {
        let gone = |generation, revision| View {
            generation,
            revision,
            sheet: None,
            rows_visited: 0,
            base: Outline { sections: Vec::new(), firsts: Vec::new(), total_lines: 0, counts: Counts::default() },
            arranged: None,
            arranged_for: None,
            visual: Visual::default(),
            lines: HashMap::new(),
        };
        let Some(sheet) = place.sheet(doc) else { return gone(generation, revision) };
        let res = Resolver::new(doc);
        match Outline::build(&res, &sheet, &Options::default()) {
            Ok(base) => View {
                generation,
                revision,
                sheet: Some(sheet),
                rows_visited: base.counts.rows,
                base,
                arranged: None,
                arranged_for: None,
                visual: Visual::default(),
                lines: HashMap::new(),
            },
            Err(_) => gone(generation, revision),
        }
    }

    /// Whether the sheet's object is gone.
    pub fn is_gone(&self) -> bool {
        self.sheet.is_none()
    }

    /// The outline of every section the sheet shows, whatever is hidden or folded: Show Sections lists these.
    pub fn base(&self) -> &Outline {
        &self.base
    }

    /// The outline as it is shown: the hidden sections left out and the folded ones as their headers.
    pub fn outline(&self) -> &Outline {
        self.arranged.as_ref().unwrap_or(&self.base)
    }

    /// Arranges the outline for `version` of the tab's hidden and folded sections and the `columns` of pairs the list is wide enough for, if it is not arranged for them yet. A new
    /// arrangement lets the lines go: their numbers changed. `first` is the visual line at the top of the view; **a change of the number of columns alone keeps the view where it
    /// was** (the line at the top is found again by the engine line it stood for). → the line the top of the view is now.
    pub fn arrange(&mut self, version: u64, hidden: &HashSet<String>, folded: &HashSet<String>, columns: usize, first: u64) -> u64 {
        if self.arranged_for == Some((version, columns)) {
            return first;
        }
        let same_sections = self.arranged_for.is_some_and(|(v, _)| v == version);
        let top = if same_sections { self.engine_lines_of(first).and_then(|(_, _, lines)| lines.first().map(|(_, engine)| *engine)) } else { None };
        if !same_sections {
            self.arranged = (!hidden.is_empty() || !folded.is_empty()).then(|| arranged(&self.base, hidden, folded));
        }
        self.visual = Visual::build(self.outline(), columns);
        self.arranged_for = Some((version, columns));
        self.lines.clear();
        top.and_then(|engine| self.outline().at(engine)).and_then(|(si, offset)| self.visual.line_of_offset(si, offset)).unwrap_or(first)
    }

    /// The visual lines: what the window scrolls, selects and paints.
    pub fn visual(&self) -> &Visual {
        &self.visual
    }

    /// The section, the kind of line and the engine lines that visual line `n` stands for, each with the column of pairs it is in.
    pub fn engine_lines_of(&self, n: u64) -> Option<(usize, VSlot, EngineLines)> {
        let (si, slot) = self.visual.at(n)?;
        Some((si, slot, self.visual.engine_lines(self.outline(), si, slot)))
    }

    /// Resolves the cells of the engine lines `from` up to `to` that are not held, and lets go of the lines far from them. → what was read. (The window reads by visual lines,
    /// `read_visual`; this is the same one run by engine line, which the tests of the cache use.)
    #[cfg(test)]
    pub fn read(&mut self, doc: &Document, from: u64, to: u64) -> Work {
        self.evict(from, to);
        self.read_run(doc, from, to)
    }

    /// Resolves the cells of the lines on view, the visual lines `from` up to `to`: only the engine lines they stand for, and those not held. → what was read.
    pub fn read_visual(&mut self, doc: &Document, from: u64, to: u64) -> Work {
        let mut wanted: Vec<u64> = Vec::new();
        for n in from..to {
            if let Some((_, _, lines)) = self.engine_lines_of(n) {
                wanted.extend(lines.into_iter().map(|(_, line)| line));
            }
        }
        wanted.sort_unstable();
        wanted.dedup();
        let (Some(low), Some(high)) = (wanted.first().copied(), wanted.last().copied()) else { return Work::default() };
        self.evict(low, high.saturating_add(1));
        let mut work = Work::default();
        let mut at = 0;
        while let Some(start) = wanted.get(at).copied() {
            let mut end = start.saturating_add(1);
            at += 1;
            while wanted.get(at).copied() == Some(end) {
                end = end.saturating_add(1);
                at += 1;
            }
            let done = self.read_run(doc, start, end);
            work.lines = work.lines.saturating_add(done.lines);
            work.cells = work.cells.saturating_add(done.cells);
        }
        work
    }

    /// Lets go of the lines far from `from` up to `to` when too many are held.
    fn evict(&mut self, from: u64, to: u64) {
        if self.lines.len() > MAX_CACHED {
            let (low, high) = (from.saturating_sub(KEEP_LINES), to.saturating_add(KEEP_LINES));
            self.lines.retain(|n, _| *n >= low && *n < high);
        }
    }

    /// Resolves the cells of the engine lines `from` up to `to` that are not held.
    fn read_run(&mut self, doc: &Document, from: u64, to: u64) -> Work {
        let mut work = Work::default();
        let Some(sheet) = self.sheet.clone() else { return work };
        let res = Resolver::new(doc);
        let mut n = from;
        while n < to {
            if self.lines.contains_key(&n) {
                n += 1;
                continue;
            }
            let mut end = n;
            while end < to && !self.lines.contains_key(&end) {
                end += 1;
            }
            let count = usize::try_from(end - n).unwrap_or(usize::MAX);
            let (read, counts) = read_lines(&res, &sheet, self.outline(), n, count);
            work.cells = work.cells.saturating_add(counts.cells);
            work.lines = work.lines.saturating_add(read.lines.len() as u64);
            if read.lines.is_empty() {
                break;
            }
            for line in read.lines {
                self.lines.insert(line.line, line);
            }
            n = end;
        }
        work
    }

    /// The engine line `n` if it has been read.
    pub fn line(&self, n: u64) -> Option<&Line> {
        self.lines.get(&n)
    }

    /// The type of the row at place `at` of the section of rows `info` (a Geometry row's: `LineTo`, `Ellipse`), read from the sheet itself. **`info` is a section of the base outline**: the row's
    /// line is found by the base numbering, so a fold or Show Sections changes nothing. One line is read and not held (a selection asks it once).
    pub fn row_type_of(&self, doc: &Document, info: &SectionInfo, at: usize) -> Option<String> {
        let sheet = self.sheet.as_ref()?;
        let overhead = 1 + info.flags.len() as u64 + u64::from(info.key.is_some());
        let line = info.first_line.checked_add(overhead)?.checked_add(u64::try_from(at).ok()?)?;
        let (read, _) = read_lines(&Resolver::new(doc), sheet, &self.base, line, 1);
        read.lines.into_iter().next().and_then(|l| l.row).and_then(|r| r.row_type)
    }

    /// The engine line a selection is on, in the outline as it is shown, if the section is shown and holds it.
    pub fn engine_line_of(&self, sel: &Sel) -> Option<u64> {
        let outline = self.outline();
        let section = outline.sections.iter().find(|s| s.name == sel.section())?;
        let overhead = 1 + section.flags.len() as u64 + u64::from(section.key.is_some());
        let at = match sel {
            Sel::Header { .. } => 0,
            Sel::Cell { row: None, cell, .. } if section.key.is_none() => 1 + section.cells.iter().position(|c| c == cell)? as u64,
            Sel::Cell { row: None, cell, .. } => 1 + section.flags.iter().position(|f| f == cell)? as u64,
            Sel::Cell { row: Some(row), .. } | Sel::Row { row, .. } => overhead + section.rows.iter().position(|r| r == row)? as u64,
        };
        // A folded section shows its header and nothing else.
        (at < section.lines).then_some(section.first_line + at)
    }

    /// The visual line a selection is on: the line of pairs a cell of a block is on, or the line of a row.
    pub fn vline_of(&self, sel: &Sel) -> Option<u64> {
        let engine = self.engine_line_of(sel)?;
        let (si, offset) = self.outline().at(engine)?;
        self.visual.line_of_offset(si, offset)
    }

    /// The place a selection is at, as a stop of its visual line: its column.
    pub fn stop_of(&self, sel: &Sel) -> Option<Stop> {
        let engine = self.engine_line_of(sel)?;
        let (si, offset) = self.outline().at(engine)?;
        let sec = self.visual.secs.get(si)?;
        let info = self.outline().sections.get(si)?;
        let in_block = offset >= 1 && offset - 1 < sec.block as u64;
        match sel {
            Sel::Header { .. } => Some(Stop { col: 0, sel: sel.clone(), strict: false }),
            Sel::Cell { row: None, .. } if in_block => {
                let (_, col) = sec.place_of(usize::try_from(offset - 1).ok()?)?;
                Some(Stop { col, sel: sel.clone(), strict: true })
            }
            Sel::Cell { row: Some(_), cell, .. } => {
                let col = info.columns.iter().position(|c| c == cell).unwrap_or(0);
                Some(Stop { col, sel: sel.clone(), strict: false })
            }
            Sel::Row { .. } | Sel::Cell { .. } => Some(Stop { col: 0, sel: sel.clone(), strict: false }),
        }
    }

    /// The places a key can stop at in visual line `n`, from the left: the cells it holds (a header is one place; the line of titles and a note are none). A row's cells are
    /// read when they are not held.
    pub fn stops(&mut self, doc: &Document, n: u64) -> Vec<Stop> {
        let Some((si, slot, engine)) = self.engine_lines_of(n) else { return Vec::new() };
        let Some(info) = self.outline().sections.get(si).cloned() else { return Vec::new() };
        match slot {
            VSlot::Header => vec![Stop { col: 0, sel: Sel::Header { section: info.name }, strict: false }],
            VSlot::Pairs(_) => engine
                .iter()
                .filter_map(|(col, line)| {
                    let offset = line.checked_sub(info.first_line)?;
                    let name = match info.at(offset)? {
                        Slot::Cell(i) => info.cells.get(i)?.clone(),
                        Slot::Flag(i) => (*info.flags.get(i)?).to_owned(),
                        _ => return None,
                    };
                    Some(Stop { col: (*col)?, sel: Sel::Cell { section: info.name.clone(), row: None, cell: name }, strict: true })
                })
                .collect(),
            VSlot::Tail(_) => {
                let Some((_, line)) = engine.first().copied() else { return Vec::new() };
                let Some(Slot::Row(i)) = line.checked_sub(info.first_line).and_then(|o| info.at(o)) else { return Vec::new() };
                let Some(row) = info.rows.get(i).cloned() else { return Vec::new() };
                self.read_run(doc, line, line + 1);
                let Some(read) = self.line(line) else { return Vec::new() };
                let mut out: Vec<Stop> = read
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(k, c)| Stop {
                        col: c.col.unwrap_or(info.columns.len() + k),
                        sel: Sel::Cell { section: info.name.clone(), row: Some(row.clone()), cell: c.name.clone() },
                        strict: false,
                    })
                    .collect();
                out.sort_by_key(|s| s.col);
                out
            }
        }
    }

    /// The selection a line has at a place of it: the cell named, or the row.
    pub fn sel_at(&self, section: usize, slot: Slot, cell: Option<&str>) -> Option<Sel> {
        let info = self.outline().sections.get(section)?;
        Some(match (slot, cell) {
            (Slot::Row(i), Some(cell)) => Sel::Cell { section: info.name.clone(), row: Some(info.rows.get(i)?.clone()), cell: cell.to_owned() },
            (Slot::Row(i), None) => Sel::Row { section: info.name.clone(), row: info.rows.get(i)?.clone() },
            (_, Some(cell)) => Sel::Cell { section: info.name.clone(), row: None, cell: cell.to_owned() },
            (_, None) => return None,
        })
    }

    /// The cell a selection names, read from the line it is on (one line is read when it is not held), with the label of its section.
    pub fn cell_of(&mut self, doc: &Document, sel: &Sel) -> Option<(String, CellView)> {
        let Sel::Cell { cell, section, .. } = sel else { return None };
        let at = self.engine_line_of(sel)?;
        if self.line(at).is_none() {
            // (What this read cost is counted by the frame that paints the line, which finds it held.)
            self.read_run(doc, at, at + 1);
        }
        let label = self.outline().sections.iter().find(|s| &s.name == section).map(|s| s.label.clone())?;
        self.line(at)?.cells.iter().find(|c| &c.name == cell).map(|c| (label, c.clone()))
    }
}

#[cfg(test)]
mod tests {
    use diacraft_engine::sheetview::Counts;

    use super::*;

    /// An outline of sections with `lines` lines each, named `A`, `B` and so on.
    fn outline(lines: &[u64]) -> Outline {
        let mut sections = Vec::new();
        let mut next = 0;
        for (i, n) in lines.iter().enumerate() {
            let name = ((b'A' + i as u8) as char).to_string();
            sections.push(SectionInfo {
                name: name.clone(),
                label: name,
                ix: None,
                display: None,
                key: None,
                cells: (1..*n).map(|c| format!("c{c}")).collect(),
                rows: Vec::new(),
                rows_total: 0,
                unlisted: 0,
                columns: Vec::new(),
                flags: Vec::new(),
                first_line: next,
                lines: *n,
                can_add_rows: false,
                can_delete: false,
            });
            next += n;
        }
        let firsts = sections.iter().map(|s| s.first_line).collect();
        Outline { sections, firsts, total_lines: next, counts: Counts::default() }
    }

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn a_hidden_section_is_left_out_and_a_folded_one_is_its_header_and_the_lines_are_numbered_again() {
        let base = outline(&[4, 3, 5]);
        let both = arranged(&base, &set(&["B"]), &set(&["C"]));
        let names: Vec<&str> = both.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "C"]);
        assert_eq!((both.sections[0].first_line, both.sections[0].lines), (0, 4));
        assert_eq!((both.sections[1].first_line, both.sections[1].lines), (4, 1), "a folded section is its header");
        assert_eq!(both.firsts, [0, 4]);
        assert_eq!(both.total_lines, 5);
        // A folded section reads only its header, wherever it is.
        assert_eq!(both.at(4), Some((1, 0)));
        assert_eq!(both.at(5), None);
        // Nothing hidden or folded is the base's own numbering.
        let same = arranged(&base, &HashSet::new(), &HashSet::new());
        assert_eq!((same.firsts.clone(), same.total_lines), (vec![0, 4, 7], 12));
        // Every section hidden leaves nothing.
        let none = arranged(&base, &set(&["A", "B", "C"]), &HashSet::new());
        assert_eq!((none.sections.len(), none.total_lines), (0, 0));
    }

    #[test]
    fn a_selection_is_on_the_line_of_its_cell_and_nowhere_when_its_section_is_hidden_or_folded() {
        let doc = Document::new_blank();
        let mut view = View::build(&doc, &Place::Document, 1, 1);
        assert!(!view.is_gone());
        view.base = outline(&[4, 3, 5]);
        view.arrange(0, &HashSet::new(), &HashSet::new(), 1, 0);
        let cell = |section: &str, cell: &str| Sel::Cell { section: section.to_owned(), row: None, cell: cell.to_owned() };
        assert_eq!(view.engine_line_of(&cell("A", "c1")), Some(1));
        assert_eq!(view.engine_line_of(&cell("C", "c3")), Some(7 + 3));
        assert_eq!(view.engine_line_of(&cell("C", "nothing")), None);
        assert_eq!(view.engine_line_of(&cell("Z", "c1")), None);
        assert_eq!(view.engine_line_of(&Sel::Header { section: "B".to_owned() }), Some(4), "a header is the first line of its section");
        view.arrange(1, &set(&["A"]), &set(&["C"]), 1, 0);
        assert_eq!(view.engine_line_of(&cell("A", "c1")), None, "a hidden section has no line");
        assert_eq!(view.engine_line_of(&cell("C", "c3")), None, "a folded section shows only its header");
        assert_eq!(view.outline().total_lines, 3 + 1);
    }

    #[test]
    fn the_visual_lines_share_a_groups_cells_in_columns_and_follow_the_arrangement() {
        let doc = Document::new_blank();
        let mut view = View::build(&doc, &Place::Document, 1, 1);
        // A: a header and 3 cells; B: a header and 2; C: a header and 4.
        view.base = outline(&[4, 3, 5]);
        view.arrange(0, &HashSet::new(), &HashSet::new(), 1, 0);
        assert_eq!(view.visual().total, 12, "one column: a line a cell, as the engine numbers them");
        view.arrange(1, &HashSet::new(), &HashSet::new(), 3, 0);
        // Three columns: A has 3 cells in one line each column, B 2 cells in one line, C 4 cells in two lines.
        assert_eq!(view.visual().secs.iter().map(|s| s.lines).collect::<Vec<_>>(), [2, 2, 3]);
        assert_eq!(view.visual().total, 7);
        let cell = |section: &str, cell: &str| Sel::Cell { section: section.to_owned(), row: None, cell: cell.to_owned() };
        assert_eq!(view.vline_of(&cell("A", "c3")), Some(1), "the three cells of A share its one line of pairs");
        assert_eq!(view.vline_of(&cell("A", "c1")), Some(1));
        assert_eq!(view.vline_of(&cell("C", "c4")), Some(6));
        assert_eq!(view.vline_of(&Sel::Header { section: "C".to_owned() }), Some(4));
        let (si, slot, engine) = view.engine_lines_of(6).unwrap();
        assert_eq!((si, slot), (2, VSlot::Pairs(1)));
        assert_eq!(
            engine,
            vec![(Some(0), 7 + 2), (Some(1), 7 + 4)],
            "C's second line holds its third cell and its fourth, which are down the first and second columns"
        );
        // A fold and Show Sections number the lines again.
        view.arrange(2, &set(&["B"]), &set(&["C"]), 3, 0);
        assert_eq!(view.visual().total, 2 + 1);
    }

    #[test]
    fn a_view_of_an_object_that_is_gone_has_no_sheet_and_reads_nothing() {
        let doc = Document::new_blank();
        let mut gone = View::build(&doc, &Place::Page(99), 1, 1);
        assert!(gone.is_gone());
        assert_eq!(gone.read(&doc, 0, 50), Work::default());
        assert_eq!(gone.outline().total_lines, 0);
        assert!(gone.line(0).is_none());
    }

    #[test]
    fn the_lines_far_from_the_view_are_let_go_when_too_many_are_held() {
        let doc = Document::new_blank();
        let mut view = View::build(&doc, &Place::Document, 1, 1);
        let total = view.outline().total_lines;
        assert!(total > 3, "the document's sheet has lines: {total}");
        let first = view.read(&doc, 0, 3);
        assert_eq!(first.lines, 3);
        // A second read of the same lines reads none.
        assert_eq!(view.read(&doc, 0, 3), Work::default());
        assert!(view.line(0).is_some() && view.line(2).is_some());
    }

    #[test]
    fn the_cells_of_the_lines_on_view_are_the_only_ones_read_whatever_the_columns() {
        let doc = Document::new_blank();
        let mut view = View::build(&doc, &Place::Document, 1, 1);
        view.arrange(0, &HashSet::new(), &HashSet::new(), 3, 0);
        let total = view.visual().total;
        assert!(total > 3);
        let first = view.read_visual(&doc, 0, 2);
        assert!(first.lines >= 2 && first.lines <= 4, "two visual lines stand for at most four engine lines: {}", first.lines);
        assert_eq!(view.read_visual(&doc, 0, 2), Work::default(), "the same lines are read once");
    }
}
