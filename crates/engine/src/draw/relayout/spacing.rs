//! Auto Space and Spacing Options (Home, Arrange, Position).
//!
//! ("makes the gaps between shapes equal and keeps their alignment as it is, on the selection or on all shapes when none is
//! selected"; Spacing Options sets the exact distance). The reading is ours where the specifications are silent (their dialog has "fields beyond the distance" they do not
//! describe):
//!
//! * the shapes are taken in **columns** (shapes whose boxes overlap along x, one after another: a column is the span of a chain of overlapping ones) and in **rows** (the same
//!   along y). A column keeps its shapes where they are in it, which is what "keeps their alignment" says;
//! * the gap between one column and the next is made the **horizontal space** and the gap between one row and the next the **vertical space**, each **`AvenueSizeX` and
//!   `AvenueSizeY` of the page** (the spacing box's "space between shapes") unless the call gives another, which is the distance the Spacing Options dialog types. The
//!   leftmost column and the topmost row stay where they are, and the others move by the change of their gap, so no shape moves past where the gaps ask;
//! * a shape moves by its own column's change along x and its own row's along y, and a shape that is where it should be is not written;
//! * with `AvoidPageBreaks` on (Position, Avoid Page Breaks) a shape that would land across a
//!   page break goes off it, as Move off Page Breaks does it;
//! * connectors glued to the shapes follow and are routed again, and callouts and the members of a container go with the shape they follow; a shape on a locked layer is left.
//!
//! **Spacing Options** ([`set_spacing`]) writes `AvenueSizeX` and `AvenueSizeY`. They are lengths in the page's drawing units, as the cells are.

use diacraft_doc::{CellAddr, CellEdit, Document, SheetRef, Value};

use super::breaks::{BREAK_MARGIN_INCHES, Breaks, delta_off, has_breaks, page_breaks};
use super::scope::{self, Piece};
use super::{Problem, carry_out, current};
use crate::draw::route::P;
use crate::draw::{checked, write_cells};
use crate::stencil::number_text;

/// What [`autospace`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spaced {
    /// The shapes spaced.
    pub shapes: usize,
    /// The shapes that moved.
    pub moved: usize,
    /// The distances used between columns and between rows, in drawing units.
    pub gap: (f64, f64),
    /// How many columns and rows the shapes made.
    pub columns: usize,
    pub rows: usize,
    /// The shapes on a locked layer that were left (the first of them).
    pub locked: Vec<u32>,
    /// How many shapes named are inside a group, and so are left to it.
    pub inside_groups: usize,
    /// Whether the document changed.
    pub changed: bool,
}

/// The tolerance of "overlap along an axis": boxes that only touch are in different columns.
const TOUCH: f64 = 1.0e-9;

/// How far each interval goes along its axis so that the gaps between the groups of overlapping intervals are `gap`: the groups are taken in order of their first edge, the first stays, and
/// each next one is put `gap` after the one before it. → the distance of each interval (in the order given) and the number of groups.
pub(super) fn spread(spans: &[(f64, f64)], gap: f64) -> (Vec<f64>, usize) {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by(|&a, &b| spans[a].0.total_cmp(&spans[b].0).then(a.cmp(&b)));
    let mut delta = vec![0.0_f64; spans.len()];
    let mut groups = 0;
    // The group being read: its first and last edge as they are now, its members, and the last edge of the group before it as it is after the move.
    let mut group: Vec<usize> = Vec::new();
    let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
    let mut placed_hi: Option<f64> = None;
    let close = |group: &mut Vec<usize>, lo: f64, hi: f64, placed_hi: &mut Option<f64>, groups: &mut usize, delta: &mut [f64]| {
        if group.is_empty() {
            return;
        }
        let shift = placed_hi.map_or(0.0, |p| (p + gap) - lo);
        for &i in group.iter() {
            delta[i] = shift;
        }
        *placed_hi = Some(hi + shift);
        *groups += 1;
        group.clear();
    };
    for i in order {
        let (a, b) = spans[i];
        if group.is_empty() {
            (lo, hi) = (a, b);
        } else if a < hi - TOUCH {
            hi = hi.max(b);
        } else {
            close(&mut group, lo, hi, &mut placed_hi, &mut groups, &mut delta);
            (lo, hi) = (a, b);
        }
        group.push(i);
    }
    close(&mut group, lo, hi, &mut placed_hi, &mut groups, &mut delta);
    (delta, groups)
}

/// The distance each shape goes: along x by its column, along y by its row (a row is read from the top, so the topmost stays), and off the page breaks when asked.
fn plan(nodes: &[Piece], gap: (f64, f64), avoid: Option<(&Breaks, f64)>) -> (Vec<P>, usize, usize) {
    let across: Vec<(f64, f64)> = nodes.iter().map(|n| (n.bounds().0, n.bounds().2)).collect();
    // Along y the topmost row stays and the others go down: the intervals are mirrored, so that the first of them is the top.
    let down: Vec<(f64, f64)> = nodes.iter().map(|n| (-n.bounds().3, -n.bounds().1)).collect();
    let (dx, columns) = spread(&across, gap.0);
    let (dy, rows) = spread(&down, gap.1);
    let mut moves: Vec<P> = dx.iter().zip(&dy).map(|(x, y)| (*x, -*y)).collect();
    if let Some((breaks, margin)) = avoid.filter(|(b, _)| has_breaks(b)) {
        for (node, d) in nodes.iter().zip(moves.iter_mut()) {
            let moved = Piece { centre: (node.centre.0 + d.0, node.centre.1 + d.1), ..node.clone() };
            if let Some(extra) = delta_off(&moved, breaks, margin) {
                *d = (d.0 + extra.0, d.1 + extra.1);
            }
        }
    }
    (moves, columns, rows)
}

/// Spaces the shapes `ids` (`None`: those of the page) as one edit. `gap` is the distance between columns and between rows in drawing units; each is the page's
/// `AvenueSizeX` or `AvenueSizeY` when it is `None`.
pub fn autospace(doc: &mut Document, page: u32, ids: Option<&[u32]>, gap: (Option<f64>, Option<f64>)) -> Result<Spaced, Problem> {
    let before = doc.clone();
    let scope = scope::read(doc, page, ids)?;
    let layout = current(doc, page)?;
    let gap = (gap.0.unwrap_or(layout.avenue.0), gap.1.unwrap_or(layout.avenue.1));
    for (name, v) in [("horizontal", gap.0), ("vertical", gap.1)] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(Problem::bad(format!("the {name} space must be a number of at least 0")));
        }
        checked(name, v)?;
    }
    let breaks = if layout.avoid_page_breaks { page_breaks(doc, page).ok() } else { None };
    let margin = BREAK_MARGIN_INCHES * crate::draw::units_per_inch(doc, page);
    let (moves, columns, rows) = plan(&scope.nodes, gap, breaks.as_ref().map(|b| (b, margin)));
    let mut paths = Vec::new();
    for (node, d) in scope.nodes.iter().zip(&moves) {
        if d.0.abs() > 1.0e-9 || d.1.abs() > 1.0e-9 {
            paths.push((node.id, node.at.clone(), *d));
        }
    }
    let out = Spaced {
        shapes: scope.nodes.len(),
        moved: paths.len(),
        gap,
        columns,
        rows,
        locked: scope.locked.clone(),
        inside_groups: scope.inside_groups,
        changed: false,
    };
    carry_out(doc, page, &before, &paths, &[], false)?;
    Ok(Spaced { changed: *doc != before, ..out })
}

/// The space between shapes the page holds, in drawing units: `AvenueSizeX` and `AvenueSizeY`, or the root style's where it holds none.
pub fn spacing_of(doc: &Document, page: u32) -> Result<(f64, f64), Problem> {
    Ok(current(doc, page)?.avenue)
}

/// Spacing Options: writes `AvenueSizeX` and/or `AvenueSizeY` (drawing units, more than 0). → whether a cell changed.
pub fn set_spacing(doc: &mut Document, page: u32, horizontal: Option<f64>, vertical: Option<f64>) -> Result<bool, Problem> {
    let have = current(doc, page)?.avenue;
    let sheet = SheetRef::Page(page);
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::new();
    for (name, wanted, held) in [("AvenueSizeX", horizontal, have.0), ("AvenueSizeY", vertical, have.1)] {
        let Some(v) = wanted else { continue };
        if !(v.is_finite() && v > 0.0) {
            return Err(Problem::bad("a space between shapes must be a number more than 0"));
        }
        checked(name, v)?;
        if (v - held).abs() > 1.0e-9 {
            writes.push((sheet.clone(), CellAddr::direct(name), CellEdit::Value(Value::from_text(&number_text(v)))));
        }
    }
    if writes.is_empty() {
        return Ok(false);
    }
    write_cells(doc, writes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_gaps_between_three_columns_are_made_equal_and_the_first_stays() {
        // Boxes 2 wide at 0..2, 3..5, 9..11 with a gap of 1 wanted: the second goes from a gap of 1 to 1 (stays), the third from 4 to 1 (goes 3 left).
        let (d, n) = spread(&[(0.0, 2.0), (3.0, 5.0), (9.0, 11.0)], 1.0);
        assert_eq!((d, n), (vec![0.0, 0.0, -3.0], 3));
    }

    #[test]
    fn shapes_that_overlap_along_the_axis_are_one_column_and_keep_their_places_in_it() {
        // Two overlapping boxes (0..4 and 1..3) are one group of span 0..4; the one at 6..8 is put 1 after it, at 5..7: a move of -1. The first group's boxes do not move.
        let (d, n) = spread(&[(0.0, 4.0), (1.0, 3.0), (6.0, 8.0)], 1.0);
        assert_eq!((d, n), (vec![0.0, 0.0, -1.0], 2));
    }

    #[test]
    fn boxes_that_only_touch_are_two_columns() {
        let (d, n) = spread(&[(0.0, 2.0), (2.0, 4.0)], 1.0);
        assert_eq!((d, n), (vec![0.0, 1.0], 2), "touching is not overlapping: the second is put a gap after the first");
    }

    #[test]
    fn the_order_the_boxes_are_given_in_does_not_matter() {
        let a = spread(&[(9.0, 11.0), (0.0, 2.0), (3.0, 5.0)], 1.0);
        assert_eq!(a, (vec![-3.0, 0.0, 0.0], 3));
    }

    #[test]
    fn one_box_and_no_box_make_no_move() {
        assert_eq!(spread(&[(3.0, 5.0)], 1.0), (vec![0.0], 1));
        assert_eq!(spread(&[], 1.0), (vec![], 0));
    }

    #[test]
    fn boxes_too_close_are_moved_apart() {
        // 0..2 and 2.2..4.2 with a gap of 1 wanted: the second goes 0.8 right.
        let (d, _) = spread(&[(0.0, 2.0), (2.2, 4.2)], 1.0);
        assert!((d[1] - 0.8).abs() < 1e-9, "{d:?}");
    }
}
