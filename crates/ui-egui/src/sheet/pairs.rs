//! How the window lays the lines of a sheet out on screen (R12 of the window's spec): **a display group is name and value pairs in several columns, filled down each column in
//! turn**, where the engine's outline has one cell a line.
//!
//! The engine numbers the lines of a sheet as `sheet.read` gives them (a header, then one cell line a cell, titles and rows), and a program reads them so. The window
//! paints **visual lines**: a group's cells (and a Geometry section's five own cells) share lines, a column of pairs holding the cells down it. How many columns there are depends on
//! the width of the list ([`pair_columns`]), so the visual lines are made again when the width changes, from the outline and nothing else: **no cell is resolved to lay them out**.
//! Scrolling, the selection and the keys all work in visual lines; the cells of the lines on view are the only ones read.

use diacraft_engine::sheetview::{Outline, line_at};

/// The least width of a pair, in points: a name right-aligned beside the box of its value.
pub const PAIR_MIN_W: f32 = 300.0;

/// The most columns of pairs (three in most of the published images).
pub const MAX_PAIR_COLUMNS: usize = 3;

/// How many columns of pairs fit a list `width` points wide: from 1 to [`MAX_PAIR_COLUMNS`], whatever the input.
pub fn pair_columns(width: f32) -> usize {
    if !width.is_finite() || width < PAIR_MIN_W {
        return 1;
    }
    (((width / PAIR_MIN_W).floor()) as usize).clamp(1, MAX_PAIR_COLUMNS)
}

/// How many cells each column of pairs holds, for a block of `cells` cells in at most `columns` columns, filled down each column in turn. The columns hold the same number of cells
/// but for the last, which holds what is left; none is empty. **Shape Transform in three columns is the one the images show**: Width, Height and Angle; PinX, PinY, LocPinX
/// and LocPinY; FlipX, FlipY and ResizeMode.
pub fn column_breaks(group: &str, cells: usize, columns: usize) -> Vec<usize> {
    if cells == 0 || columns == 0 {
        return Vec::new();
    }
    if group == "Shape Transform" && cells == 10 && columns == 3 {
        return vec![3, 4, 3];
    }
    let columns = columns.min(cells);
    let rows = cells.div_ceil(columns);
    let mut left = cells;
    let mut out = Vec::with_capacity(columns);
    while left > 0 {
        let take = rows.min(left);
        out.push(take);
        left -= take;
    }
    out
}

/// The engine lines a visual line stands for, each with the column of pairs it is in (none for a line that is not a line of pairs).
pub type EngineLines = Vec<(Option<usize>, u64)>;

/// What a visual line is, within its section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VSlot {
    Header,
    /// The nth line of pairs of the block.
    Pairs(u64),
    /// The nth engine line after the block (0 is the line of titles of a section of rows, then its rows, then its note).
    Tail(u64),
}

/// One section of the visual lines.
#[derive(Clone, Debug, Default)]
pub struct VSec {
    /// The first visual line of the section.
    pub first: u64,
    /// How many visual lines it has.
    pub lines: u64,
    /// How many cells each column of pairs holds (empty for a folded section).
    pub breaks: Vec<usize>,
    /// The cells of the block: a group's, or a section of rows' own cells.
    pub block: usize,
    /// The visual lines of the block.
    pub rows: u64,
}

impl VSec {
    /// The index of the block's cell at line `r` of column `c`, if there is one.
    pub fn cell_at(&self, r: u64, c: usize) -> Option<usize> {
        let held = *self.breaks.get(c)?;
        if r >= held as u64 {
            return None;
        }
        let before: usize = self.breaks.iter().take(c).sum();
        before.checked_add(usize::try_from(r).ok()?)
    }

    /// Where the block's cell `index` is: its line of pairs and its column.
    pub fn place_of(&self, index: usize) -> Option<(u64, usize)> {
        let mut before = 0_usize;
        for (c, held) in self.breaks.iter().enumerate() {
            if index < before.saturating_add(*held) {
                return Some(((index - before) as u64, c));
            }
            before = before.saturating_add(*held);
        }
        None
    }
}

/// The visual lines of a sheet.
#[derive(Clone, Debug, Default)]
pub struct Visual {
    pub columns: usize,
    pub secs: Vec<VSec>,
    /// The first visual line of each section: strictly rising.
    pub firsts: Vec<u64>,
    pub total: u64,
}

impl Visual {
    /// The visual lines of `outline` (as the window shows it: a hidden section is gone, a folded one is its header) in `columns` columns of pairs.
    pub fn build(outline: &Outline, columns: usize) -> Visual {
        let mut secs: Vec<VSec> = Vec::with_capacity(outline.sections.len());
        let mut next = 0_u64;
        for sec in &outline.sections {
            let block = if sec.key.is_none() { sec.cells.len() } else { sec.flags.len() };
            let mut out = VSec { first: next, lines: 1, breaks: Vec::new(), block, rows: 0 };
            // A folded section is its header, and a section with nothing after it is one too.
            if sec.lines > 1 {
                let breaks = column_breaks(&sec.name, block, columns);
                let rows = breaks.iter().copied().max().unwrap_or(0) as u64;
                let tail = sec.lines.saturating_sub(1).saturating_sub(block as u64);
                out.lines = 1u64.saturating_add(rows).saturating_add(tail);
                out.rows = rows;
                out.breaks = breaks;
            }
            next = next.saturating_add(out.lines);
            secs.push(out);
        }
        let firsts = secs.iter().map(|s| s.first).collect();
        Visual { columns, secs, firsts, total: next }
    }

    /// The section that holds visual line `n` and what the line is there, by a binary search of the sections' first lines. `None` past the last line.
    pub fn at(&self, n: u64) -> Option<(usize, VSlot)> {
        let (si, offset) = line_at(&self.firsts, self.total, n)?;
        let sec = self.secs.get(si)?;
        Some((
            si,
            if offset == 0 {
                VSlot::Header
            } else if offset <= sec.rows {
                VSlot::Pairs(offset - 1)
            } else {
                VSlot::Tail(offset - 1 - sec.rows)
            },
        ))
    }

    /// The engine lines that visual line `n` of section `si` stands for, in the order of its columns: each with the column of pairs it is in, or none for a line that is not
    /// a line of pairs. `outline` is the one the visual lines were made from.
    pub fn engine_lines(&self, outline: &Outline, si: usize, slot: VSlot) -> EngineLines {
        let (Some(sec), Some(info)) = (self.secs.get(si), outline.sections.get(si)) else { return Vec::new() };
        match slot {
            VSlot::Header => vec![(None, info.first_line)],
            VSlot::Pairs(r) => (0..sec.breaks.len())
                .filter_map(|c| sec.cell_at(r, c).map(|i| (Some(c), info.first_line.saturating_add(1).saturating_add(i as u64))))
                .collect(),
            VSlot::Tail(t) => vec![(None, info.first_line.saturating_add(1).saturating_add(sec.block as u64).saturating_add(t))],
        }
    }

    /// The visual line of the engine line at `offset` of section `si`.
    pub fn line_of_offset(&self, si: usize, offset: u64) -> Option<u64> {
        let sec = self.secs.get(si)?;
        if offset == 0 {
            return Some(sec.first);
        }
        let within = offset - 1;
        if within < sec.block as u64 {
            let (r, _) = sec.place_of(usize::try_from(within).ok()?)?;
            return sec.first.checked_add(1)?.checked_add(r);
        }
        let tail = within - sec.block as u64;
        let at = sec.first.checked_add(1)?.checked_add(sec.rows)?.checked_add(tail)?;
        (at < sec.first.checked_add(sec.lines)?).then_some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_columns_of_pairs_are_one_to_three_by_the_width_for_any_input() {
        assert_eq!(pair_columns(100.0), 1);
        assert_eq!(pair_columns(299.9), 1);
        assert_eq!(pair_columns(600.0), 2);
        assert_eq!(pair_columns(900.0), 3);
        assert_eq!(pair_columns(5000.0), MAX_PAIR_COLUMNS);
        for odd in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -3.0, 0.0] {
            assert_eq!(pair_columns(odd), 1, "{odd}");
        }
    }

    #[test]
    fn a_block_fills_down_each_column_in_turn_and_shape_transform_is_three_four_and_three() {
        assert_eq!(column_breaks("Shape Transform", 10, 3), [3, 4, 3], "The way the images lay Shape Transform out");
        assert_eq!(column_breaks("Shape Transform", 10, 2), [5, 5]);
        assert_eq!(column_breaks("Shape Transform", 10, 1), [10]);
        assert_eq!(column_breaks("Protection", 11, 3), [4, 4, 3]);
        assert_eq!(column_breaks("Protection", 5, 3), [2, 2, 1]);
        assert_eq!(column_breaks("Geometry", 5, 3), [2, 2, 1]);
        assert_eq!(column_breaks("X", 2, 3), [1, 1], "no column is empty");
        assert_eq!(column_breaks("X", 1, 3), [1]);
        assert_eq!(column_breaks("X", 0, 3), Vec::<usize>::new());
        assert_eq!(column_breaks("X", 4, 0), Vec::<usize>::new());
        for cells in 0..40 {
            for columns in 0..5 {
                let breaks = column_breaks("Group", cells, columns);
                let (sum, empty) = (breaks.iter().sum::<usize>(), breaks.contains(&0));
                assert_eq!(
                    (sum == if columns == 0 { 0 } else { cells }, empty, breaks.len() <= columns.max(1)),
                    (true, false, true),
                    "{cells} in {columns}: {breaks:?}"
                );
            }
        }
    }

    #[test]
    fn a_blocks_cells_are_found_by_their_line_and_column_and_back() {
        let sec = VSec { first: 0, lines: 5, breaks: vec![3, 4, 3], block: 10, rows: 4 };
        assert_eq!(sec.cell_at(0, 0), Some(0));
        assert_eq!(sec.cell_at(2, 0), Some(2));
        assert_eq!(sec.cell_at(3, 0), None, "the first column holds three");
        assert_eq!(sec.cell_at(0, 1), Some(3));
        assert_eq!(sec.cell_at(3, 1), Some(6));
        assert_eq!(sec.cell_at(0, 2), Some(7));
        assert_eq!(sec.cell_at(0, 3), None);
        for index in 0..10 {
            let (r, c) = sec.place_of(index).unwrap();
            assert_eq!(sec.cell_at(r, c), Some(index));
        }
        assert_eq!(sec.place_of(10), None);
    }
}
