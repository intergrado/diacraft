//! Auto Size: a page that grows by itself, a sheet of its printer paper at a time, when a shape is put or moved past its edge.
//!
//! "with Auto Size on, working past the edge extends the page, page breaks show the sheet
//! divisions, and paper size and orientation set those divisions", and V1: "given Auto Size on, Letter paper and a shape dragged past the right
//! edge, expect the page to extend past its old edge ... and the paper size to stay Letter".
//!
//! **Where it hooks.** [`fit_page`] is called by the commands that put a shape somewhere or change where it reaches, inside the command's own
//! edit, so the growth is part of its one undo step: `shape.drop` (and a drop from a stencil file), `shape.move`, `shape.resize`, `shape.rotate`,
//! `shape.flip`, `shape.align`, `edit.paste`, `shape.duplicate`, `text.box` and `connector.autoconnect`. It is given the shapes the command named or made, and no other: a shape that
//! was past the edge before is left where it is, and the page does not grow for it.
//!
//! Our rule. The page grows by **whole sheets** of its printer paper: one sheet is the paper (`PaperKind`) held the way
//! `PrintPageOrientation` says, on paper, in drawing units (times the page's drawing units per paper inch). A page that says nothing of the way
//! its sheet is held has it held the way the page is when it first grows, and that growth writes `PrintPageOrientation`, so that the sheets do
//! not turn as the page grows taller than wide. On each side that a shape's box passes
//! (its four corners carried through its turn and its groups), the page gains as many sheets as take it in: `ceil(overshoot / sheet)`. A shape that
//! only touches the edge does not make the page grow.
//!
//! Our rule. A page's origin is its lower-left corner, so growing on the **left or bottom** moves the origin:
//! every top-level shape is carried by the growth (the shapes of groups go with their groups), as a move of all of them is, and the shape that
//! made the page grow ends inside it. They are carried together, so the drawing is not changed: connector routes are not made again, a glued
//! end follows its shape, and what reads a pose cell is worked out again (the way a paste or a group does, `draw::settle::settle_kept`). Growing on
//! the right or the top moves nothing.
//!
//! **What it costs.** With Auto Size off the hook reads two cells. With it on, it makes one walk of the page to find the shapes (`scan`), reads
//! their boxes, and writes two cells (three the first time: `PrintPageOrientation`) when the page grows. A shift is linear in the top-level shapes and the `Connect` rows, once, and is made
//! only for the growth that needs it; a page of more than [`MAX_SHIFT_SHAPES`] top-level shapes is not shifted, so it does not grow on the left or
//! the bottom (the shape stays off the page) and still grows on the other sides.
//!
//! **Limits.** A page does not grow past [`MAX_PAGE_INCHES`] on paper or [`MAX_COORD`] drawing units on a side: a shape that would need more is
//! left off the page, and the command still succeeds. A growth that cannot be made (the page size or a shape's pin is protected by `GUARD`, a
//! number is out of range) is dropped and the command goes on without it.

use std::collections::HashSet;

use diacraft_calc::Changes;
use diacraft_doc::{CellAddr, CellEdit, CellWrite, Document, Owner, ShapeRef, SheetRef, Value, WriteOrigin, WriteOutcome};

use super::group::glued_ends;
use super::lift::{Affine, Lift, lift};
use super::page_setup::{MAX_PAGE_INCHES, auto_size_on, geometry};
use super::read::Reader;
use super::route::P;
use super::scan::scan;
use super::settle::settle_kept;
use super::{MAX_COORD, Problem};
use crate::stencil::number_text;

/// Most top-level shapes a growth on the left or the bottom carries.
pub const MAX_SHIFT_SHAPES: usize = 20_000;

/// A shape's box within this many drawing units of the page's edge is not past it.
const EDGE: f64 = 1.0e-6;

/// How many sheets to add on each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sheets {
    left: u32,
    right: u32,
    bottom: u32,
    top: u32,
}

/// How many whole sheets of `sheet` take in `over` (drawing units past an edge): none for nothing, and `u32::MAX` for more than anything may have.
fn sheets_for(over: f64, sheet: f64) -> u32 {
    if over.is_nan() || over <= EDGE {
        return 0;
    }
    let n = ((over - EDGE) / sheet).ceil();
    if n.is_finite() && n <= f64::from(u32::MAX) { n as u32 } else { u32::MAX }
}

/// The sheets to add before and after on one axis, none when the page would pass its limits.
fn axis(low: f64, high: f64, length: f64, (sheet, k): (f64, f64), may_shift: bool) -> (u32, u32) {
    let before = if may_shift { sheets_for(-low, sheet) } else { 0 };
    let after = sheets_for(high - length, sheet);
    if before == 0 && after == 0 {
        return (0, 0);
    }
    let total = length + (f64::from(before) + f64::from(after)) * sheet;
    if total / k > MAX_PAGE_INCHES || total > MAX_COORD { (0, 0) } else { (before, after) }
}

/// The box that holds the boxes of the shapes in `found` on the page: (lowest x, lowest y) and (highest x, highest y). `None` for none.
fn bounds<'a>(reader: &Reader<'_>, found: impl Iterator<Item = &'a ShapeRef>) -> Option<(P, P)> {
    let mut out: Option<(P, P)> = None;
    for at in found {
        let corners = reader.corners(at);
        if corners.iter().any(|(x, y)| !(x.is_finite() && y.is_finite())) {
            continue;
        }
        for (x, y) in corners {
            out = Some(match out {
                None => ((x, y), (x, y)),
                Some((lo, hi)) => ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y))),
            });
        }
    }
    out
}

/// What the page needs to take in the shapes `ids`: the sheets to add on each side and one sheet's size. `None` when Auto Size is off
/// or the page needs nothing.
fn plan(doc: &Document, page: u32, ids: &[u32], whatever_auto_size_says: bool) -> Option<(Sheets, (f64, f64))> {
    if ids.is_empty() {
        return None;
    }
    let reader = Reader::new(doc);
    if !whatever_auto_size_says && !auto_size_on(&reader, &SheetRef::Page(page)) {
        return None;
    }
    let g = geometry(doc, &reader, page).ok()?;
    let (sheet_w, sheet_h) = g.tile();
    if !(sheet_w.is_finite() && sheet_h.is_finite() && sheet_w > 0.0 && sheet_h > 0.0) {
        return None;
    }
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    let found = scan(doc, page, &wanted).ok()?;
    let (low, high) = bounds(&reader, found.found.values())?;
    let may_shift = doc.page(page)?.shapes.len() <= MAX_SHIFT_SHAPES;
    let (left, right) = axis(low.0, high.0, g.width, (sheet_w, g.k), may_shift);
    let (bottom, top) = axis(low.1, high.1, g.height, (sheet_h, g.k), may_shift);
    let sheets = Sheets { left, right, bottom, top };
    (sheets != Sheets { left: 0, right: 0, bottom: 0, top: 0 }).then_some((sheets, (sheet_w, sheet_h)))
}

/// Carries every top-level shape of the page by (`dx`, `dy`). → the cells written, for the recalculation.
fn shift_contents(doc: &mut Document, page: u32, (dx, dy): P) -> Result<Vec<(SheetRef, CellAddr)>, Problem> {
    let tops = doc.page(page).ok_or_else(|| Problem::failed("the page went away"))?.shapes.len();
    if tops > MAX_SHIFT_SHAPES {
        return Err(Problem::failed("the page holds too many shapes to be carried"));
    }
    // A shape held to guides is carried by its guides, which are carried: its pins are left to them.
    let (held, mut cells) = crate::snap::hold::shift_holds(doc, page, (dx, dy))?;
    let mut items: Vec<Lift> = Vec::with_capacity(tops);
    if let Some(p) = doc.page(page) {
        for (place, shape) in p.shapes.iter().enumerate() {
            let place = u32::try_from(place).map_err(|_| Problem::failed("too many shapes"))?;
            if held.contains(&shape.id) {
                continue;
            }
            items.push(Lift { id: shape.id, at: ShapeRef { owner: Owner::Page(page), path: vec![place] }, by: Affine::translation(dx, dy) });
        }
    }
    let ids: HashSet<u32> = items.iter().map(|item| item.id).collect();
    let glued = glued_ends(doc, page, &ids);
    let written = lift(doc, &items, &glued)?;
    for (item, names) in items.iter().zip(&written) {
        let sheet = SheetRef::Shape(item.at.clone());
        cells.extend(names.iter().map(|name| (sheet.clone(), CellAddr::direct(name))));
    }
    Ok(cells)
}

/// Makes the growth `sheets` of sheets `sheet` big. → whether the page grew (not when its size is protected by `GUARD`).
fn grow(doc: &mut Document, page: u32, sheets: Sheets, sheet: (f64, f64)) -> Result<bool, Problem> {
    let g = geometry(doc, &Reader::new(doc), page)?;
    let across = f64::from(sheets.left) + f64::from(sheets.right);
    let down = f64::from(sheets.bottom) + f64::from(sheets.top);
    let size = SheetRef::Page(page);
    let write = |name: &str, value: f64| {
        let edit = CellEdit::Value(Value::from_text(&number_text(value)));
        (size.clone(), CellWrite { addr: CellAddr::direct(name), edit, origin: WriteOrigin::Drawing })
    };
    let mut writes = vec![("PageWidth", g.width + across * sheet.0), ("PageHeight", g.height + down * sheet.1)];
    // **The way the sheet is held is fixed by the first growth**: a page whose sheet is held "the way the page is" would otherwise turn its
    // sheets as it grows (11 by 8.5 becomes 11 by 17, which is taller than wide, and its sheets would be 8.5 by 11).
    if !g.paper.orientation_set {
        writes.push(("PrintPageOrientation", g.paper.orientation.print_code()));
    }
    let outcomes = doc.write_batch(writes.iter().map(|(name, value)| write(name, *value)).collect())?;
    if outcomes.contains(&WriteOutcome::KeptGuarded) {
        return Ok(false);
    }
    let mut cells: Vec<(SheetRef, CellAddr)> = writes.iter().map(|(name, _)| (size.clone(), CellAddr::direct(name))).collect();
    let carry = (f64::from(sheets.left) * sheet.0, f64::from(sheets.bottom) * sheet.1);
    if carry != (0.0, 0.0) {
        cells.extend(shift_contents(doc, page, carry)?);
    }
    settle_kept(doc, page, &Changes { cells, ..Changes::default() })?;
    Ok(true)
}

/// Grows the page for the shapes `ids` if it has Auto Size on and they pass its edge. → whether it grew.
///
/// A growth that cannot be made does not fail the command that asked: the page is as it was, and the shape sits off the page as it does when
/// Auto Size is off. That is the one place an error here is let go, and it is right: refusing a drop because a shape elsewhere on the page is
/// protected from moving would be worse than a shape off the page.
pub fn fit_page(doc: &mut Document, page: u32, ids: &[u32]) -> Result<bool, Problem> {
    grow_for(doc, page, ids, false)
}

/// Enlarges the page to take in the shapes `ids` whatever Auto Size says (ours: `ResizePage` 1 makes a layout "enlarge the page to enclose the drawing when it's
/// done", and the page grows as Auto Size grows it, by whole sheets of its printer paper). → whether it grew.
pub fn enlarge_page(doc: &mut Document, page: u32, ids: &[u32]) -> Result<bool, Problem> {
    grow_for(doc, page, ids, true)
}

fn grow_for(doc: &mut Document, page: u32, ids: &[u32], whatever_auto_size_says: bool) -> Result<bool, Problem> {
    let Some((sheets, sheet)) = plan(doc, page, ids, whatever_auto_size_says) else { return Ok(false) };
    let backup = doc.clone();
    match grow(doc, page, sheets, sheet) {
        Ok(grew) => {
            if !grew {
                *doc = backup;
            }
            Ok(grew)
        }
        Err(_) => {
            *doc = backup;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn whole_sheets_take_in_what_is_past_the_edge() {
        assert_eq!(sheets_for(0.0, 11.0), 0);
        assert_eq!(sheets_for(-3.0, 11.0), 0);
        assert_eq!(sheets_for(1.0e-7, 11.0), 0, "a box that only touches the edge does not grow the page");
        assert_eq!(sheets_for(0.01, 11.0), 1);
        assert_eq!(sheets_for(11.0, 11.0), 1, "exactly one sheet past the edge is one sheet");
        assert_eq!(sheets_for(11.000_1, 11.0), 2);
        assert_eq!(sheets_for(14.75, 11.0), 2);
        assert_eq!(sheets_for(f64::NAN, 11.0), 0);
        assert_eq!(sheets_for(f64::INFINITY, 11.0), u32::MAX);
        assert_eq!(sheets_for(1.0e300, 11.0), u32::MAX);
    }

    #[test]
    fn an_axis_grows_on_the_sides_a_box_passes_and_stops_at_the_limits() {
        // A Letter landscape page, 11 wide: a box from x 12 to 13.5 needs one sheet on the right.
        assert_eq!(axis(12.0, 13.5, 11.0, (11.0, 1.0), true), (0, 1));
        // From x -1.5 to 5: one on the left.
        assert_eq!(axis(-1.5, 5.0, 11.0, (11.0, 1.0), true), (1, 0));
        // Both sides.
        assert_eq!(axis(-1.0, 12.0, 11.0, (11.0, 1.0), true), (1, 1));
        // The left is not carried on a page too full to shift: only the right grows.
        assert_eq!(axis(-1.0, 12.0, 11.0, (11.0, 1.0), false), (0, 1));
        assert_eq!(axis(-1.0, 5.0, 11.0, (11.0, 1.0), false), (0, 0));
        // Inside: nothing.
        assert_eq!(axis(1.0, 10.0, 11.0, (11.0, 1.0), true), (0, 0));
        // Past the limit on paper (10,000 inches) or in drawing units (1,000,000): left off the page.
        assert_eq!(axis(0.0, 20_000.0, 11.0, (11.0, 1.0), true), (0, 0));
        assert_eq!(axis(0.0, 2_000_000.0, 11.0, (11.0, 100.0), true), (0, 0), "9,000 inches, but more than 1,000,000 drawing units");
        assert_eq!(axis(0.0, f64::INFINITY, 11.0, (11.0, 1.0), true), (0, 0));
    }

    #[test]
    fn the_box_of_the_shapes_holds_their_corners() {
        let doc = Document::new_blank();
        let reader = Reader::new(&doc);
        assert_eq!(bounds(&reader, std::iter::empty()), None);
    }

    #[test]
    fn the_hook_on_a_page_of_a_hundred_thousand_shapes_is_one_walk_of_it() {
        // Counted, not timed (`tests/it/page_setup_cost.rs` held this to seconds): with Auto Size on, finding where the new shape is takes one walk
        // of the page, a visit to each shape, and not a walk for each of the shapes asked about or a search from the top of the page.
        let n = 100_000_u32;
        let mut doc = Document::new_blank();
        if let Some(page) = crate::draw::page_mut(&mut doc, 1) {
            for i in 0..n {
                page.shapes.push(std::sync::Arc::new(diacraft_doc::Shape::new(i + 1)));
            }
        }
        let on = CellEdit::Value(Value::from_text("1"));
        doc.write_cell(&SheetRef::Page(1), &CellAddr::direct("DrawingResizeType"), on, WriteOrigin::Drawing).unwrap();
        crate::draw::scan::NODES_WALKED.with(|c| c.set(0));
        crate::draw::read::FINDS.with(|c| c.set(0));
        let grew = fit_page(&mut doc, 1, &[n]).unwrap();
        assert!(!grew, "the new shape is inside the page, so it does not grow");
        assert_eq!(crate::draw::scan::NODES_WALKED.with(std::cell::Cell::get), n as usize, "one visit to each shape of the page");
        assert_eq!(crate::draw::read::FINDS.with(std::cell::Cell::get), 0, "no search from the top of the page for the shape");
    }
}
