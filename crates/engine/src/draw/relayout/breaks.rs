//! Page breaks: where the drawing passes the printer paper (View, Show, Page Breaks; Home, Arrange, Position, Move off Page Breaks).
//!
//! The "a dashed line marks the border between tiles, each of which is the part of the drawing that prints on one sheet". The
//! breaks are the borders between the sheets a print of the page takes: the tiles `diacraft_render::print::plan_page` works out from the page's `Print Properties` cells (the
//! paper, the way it is held, the margins, the print scale, the centring), the same tiles Print Preview and Print use, so what the window marks is what the printer is given. A
//! page that is one sheet has none. [`page_breaks`] gives the places along x and along y of the lines, **in drawing units** (the places of the page's cells).
//!
//! The reading is ours: "it moves any shape that crosses a printed page break off it". [`move_off_breaks`] moves each shape
//! of the scope that a break runs across to the side of the break it is nearer, along that axis only, until the break is [`BREAK_MARGIN_INCHES`] away from it, so a shape
//! stays on one sheet. A shape too large for one sheet cannot be moved off and is counted in [`MovedOff::too_large`] and left. Shapes on a locked layer are left, and listed.

use diacraft_doc::Document;

use super::scope::{self, Piece};
use super::{Problem, carry_out};
use crate::draw::route::P;

/// How far from a break a shape is put when it is moved off it, in paper inches.
pub const BREAK_MARGIN_INCHES: f64 = 0.05;

/// The tolerance of "the break runs across the shape": a shape whose edge is this near a break only touches it, in drawing units.
pub(crate) const TOUCH: f64 = 1.0e-6;

/// The lines of the page breaks, in drawing units, each list in ascending order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Breaks {
    /// The `x` of each vertical line.
    pub x: Vec<f64>,
    /// The `y` of each horizontal line.
    pub y: Vec<f64>,
    /// How far the sheets reach along x and along y: the outer edges of the first and the last (the walls a shape moved off a break may not pass).
    pub x_span: (f64, f64),
    pub y_span: (f64, f64),
}

/// Where the breaks of page `page` fall. `Err` in words for a page that cannot be printed as it is (it has no size, its margins leave no printable area, or it needs more
/// sheets than a print takes): a window draws no break for it.
pub fn page_breaks(doc: &Document, page: u32) -> Result<Breaks, Problem> {
    let plan = diacraft_render::print::plan_page(doc, page).map_err(|e| Problem::failed(e.to_string()))?;
    let k = crate::draw::units_per_inch(doc, page);
    let mut breaks = Breaks { x_span: (f64::INFINITY, f64::NEG_INFINITY), y_span: (f64::INFINITY, f64::NEG_INFINITY), ..Breaks::default() };
    for sheet in &plan.sheets {
        let rect = sheet.clip_rect_in_page();
        breaks.x_span = (breaks.x_span.0.min(rect.x0 * k), breaks.x_span.1.max(rect.x1 * k));
        breaks.y_span = (breaks.y_span.0.min(rect.y0 * k), breaks.y_span.1.max(rect.y1 * k));
        if sheet.row == 0 && sheet.col > 0 {
            breaks.x.push(rect.x0 * k);
        }
        if sheet.col == 0 && sheet.row > 0 {
            breaks.y.push(rect.y1 * k);
        }
    }
    breaks.x.sort_by(f64::total_cmp);
    breaks.y.sort_by(f64::total_cmp);
    Ok(breaks)
}

/// How far an interval `[lo, hi]` must move along its axis to lie on one sheet (between two breaks, or a break and an outer edge in `span`, not across a break): 0 when no break
/// runs across it, the shorter way when two ways are open, and `None` when it cannot be (it is longer than the sheet it would go to). `margin` is the clear space left to the break.
pub(crate) fn off_breaks(lo: f64, hi: f64, breaks: &[f64], span: (f64, f64), margin: f64) -> Option<f64> {
    let across = |a: f64, b: f64| breaks.iter().any(|&c| a + TOUCH < c && c < b - TOUCH);
    let on_page = |a: f64, b: f64| a >= span.0 - TOUCH && b <= span.1 + TOUCH;
    let Some(&c) = breaks.iter().find(|&&c| lo + TOUCH < c && c < hi - TOUCH) else { return Some(0.0) };
    let (left, right) = ((c - margin) - hi, (c + margin) - lo);
    let mut best: Option<f64> = None;
    for d in [left, right] {
        if !across(lo + d, hi + d) && on_page(lo + d, hi + d) && best.is_none_or(|b| d.abs() < b.abs()) {
            best = Some(d);
        }
    }
    best
}

/// What [`move_off_breaks`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MovedOff {
    /// The shapes looked at.
    pub shapes: usize,
    /// The shapes that moved.
    pub moved: usize,
    /// The shapes a break runs across that are too large for one sheet, and were left.
    pub too_large: usize,
    /// The shapes on a locked layer that were left (the first of them).
    pub locked: Vec<u32>,
    /// How many shapes named are inside a group, and so are left to it.
    pub inside_groups: usize,
    /// Whether the document changed.
    pub changed: bool,
}

/// Moves the shapes `ids` (`None`: those of the page) that a page break runs across off it, as one edit: connectors glued to them follow, and callouts and
/// the members of a container go along.
pub fn move_off_breaks(doc: &mut Document, page: u32, ids: Option<&[u32]>) -> Result<MovedOff, Problem> {
    let before = doc.clone();
    let scope = scope::read(doc, page, ids)?;
    let breaks = page_breaks(doc, page)?;
    let margin = BREAK_MARGIN_INCHES * crate::draw::units_per_inch(doc, page);
    let mut out = MovedOff { shapes: scope.nodes.len(), locked: scope.locked.clone(), inside_groups: scope.inside_groups, ..MovedOff::default() };
    let mut paths: Vec<(u32, diacraft_doc::ShapeRef, P)> = Vec::new();
    for node in &scope.nodes {
        match delta_off(node, &breaks, margin) {
            Some(d) if d != (0.0, 0.0) => paths.push((node.id, node.at.clone(), d)),
            Some(_) => {}
            None => out.too_large += 1,
        }
    }
    out.moved = paths.len();
    carry_out(doc, page, &before, &paths, &[], false)?;
    out.changed = *doc != before;
    Ok(out)
}

/// How far a shape goes along x and along y to be off the breaks, `None` when it is across one it cannot be moved off.
pub(super) fn delta_off(node: &Piece, breaks: &Breaks, margin: f64) -> Option<P> {
    let (x0, y0, x1, y1) = node.bounds();
    Some((off_breaks(x0, x1, &breaks.x, breaks.x_span, margin)?, off_breaks(y0, y1, &breaks.y, breaks.y_span, margin)?))
}

/// Whether the shapes were considered at all: when the page has no break, none moves.
pub(super) fn has_breaks(b: &Breaks) -> bool {
    !b.x.is_empty() || !b.y.is_empty()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A page as wide as any: no wall but the breaks.
    const WIDE: (f64, f64) = (-1.0e9, 1.0e9);

    #[test]
    fn an_interval_across_a_break_goes_to_the_nearer_side() {
        // Breaks at 10 and 20; a shape from 9 to 12 is across the first: left puts its right edge at 10 - 0.5 (a move of -2.5), right puts its left edge at 10.5 (1.5).
        assert_eq!(off_breaks(9.0, 12.0, &[10.0, 20.0], WIDE, 0.5), Some(1.5));
        assert_eq!(off_breaks(8.0, 10.25, &[10.0, 20.0], WIDE, 0.5), Some(-0.75));
    }

    #[test]
    fn an_interval_that_only_touches_a_break_or_is_between_breaks_stays() {
        assert_eq!(off_breaks(8.0, 10.0, &[10.0], WIDE, 0.5), Some(0.0), "touching is not across");
        assert_eq!(off_breaks(10.0, 12.0, &[10.0], WIDE, 0.5), Some(0.0));
        assert_eq!(off_breaks(11.0, 15.0, &[10.0, 20.0], WIDE, 0.5), Some(0.0));
        assert_eq!(off_breaks(1.0, 5.0, &[], WIDE, 0.5), Some(0.0), "a page with no breaks has nothing to avoid");
    }

    #[test]
    fn an_interval_longer_than_the_space_between_two_breaks_cannot_be_moved_off() {
        // 12 long between breaks 10 apart, on a page that reaches from 0 to 30: left of the first break is a sheet 10 wide, right of it is across the next break.
        assert_eq!(off_breaks(9.0, 21.0, &[10.0, 20.0], (0.0, 30.0), 0.5), None);
        // It would fit with no margin to spare and does not with one.
        assert_eq!(off_breaks(9.5, 19.8, &[10.0, 20.0], (0.0, 30.0), 0.5), None);
        // A shape that would go off the page's edge may not: 9 long from 8 to 17 across the break at 10 of a page from 0 to 18: right is the sheet from 10 to 18, which is 8 wide.
        assert_eq!(off_breaks(8.0, 17.0, &[10.0], (0.0, 18.0), 0.5), Some(-7.5), "left is the sheet from 0 to 10, which holds it");
    }

    #[test]
    fn a_move_that_would_put_the_shape_across_the_next_break_goes_the_other_way() {
        // Breaks at 10, 12 and 30; the shape is [9, 13]. Right is the shorter way (1.5) and lands on [10.5, 14.5], across the break at 12; left (-3.5) lands on [5.5, 9.5].
        assert_eq!(off_breaks(9.0, 13.0, &[10.0, 12.0, 30.0], WIDE, 0.5), Some(-3.5));
    }
}
