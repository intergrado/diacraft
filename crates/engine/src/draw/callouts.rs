//! Callouts: the ties that tie a callout to a shape, adding one from the gallery, moving its leader's end, and what moves, copies and deletes with the shape it is tied to
//! The shapes of the gallery are `stencil/callout.rs`'s.
//!
//! The reading is, the upkeep is ours. A callout and the shape it points at hold the tie **on both sides**, in their `Relationships` cells, as the files do: the
//! callout's `DEPENDSON(6, Sheet.<target>!SheetRef())` and the target's `DEPENDSON(3, Sheet.<callout>!SheetRef(), ...)`. A call this module does not own (the ties of containers and lists, codes 1, 2, 4, 5 and
//! 7 to 11) is kept as it is, and a target it cannot read as `Sheet.<id>` is kept as the text it was. Tying, untying, a copy and a delete change both cells, so stays true. A shape may have at most
//! [`MAX_CALLOUTS_PER_SHAPE`] callouts.
//!
//! A shape that moves takes the callouts tied to it, and theirs, along by the same distance on the page ("it moves with the shape"), in the same undo step: the move
//! commands, nudges, the pin of the Size & Position window and Align all reach it ([`followers`]). A callout that moves on its own leaves the target where it is, and its leader's end stays on the target, so
//! the line gets longer or shorter. A callout moved with its target (both selected) moves once. At most [`MAX_FOLLOWING`] callouts follow one edit, and a move that would take more is refused in words.
//!
//! A copy of a shape takes the callouts tied to it that are on the page's top level, and a paste ties each copy to the copy of its target ("copying S copies the callout").
//! A callout copied **alone** is a callout tied to nothing: the copy holds its leader's end where it was in its own frame (a paste is a move of the copies together, so the end is where it was relative to the
//! callout), and the original keeps its tie (the specifications do not say; the choice loses nothing). A shape copied that is tied to a callout not copied is tied to nothing.
//!
//! Deleting a shape deletes the callouts tied to it, and theirs ("deleting S deletes it"). Deleting a callout takes its name out of its target's `Relationships`, so that no cell names
//! a shape that is not there (an `ID` may be given again by a later drawing). A cut is a copy and a delete.
//!
//! Our rule. A callout is tied to a shape of the page's top level only, and is itself on it: the end of the leader is worked out from the target's `PinX` and `PinY`, which are in the frame of its
//! parent, and the callout's own pin is in another if the two are not in the same list. A shape inside a group is not a target (tie the group), and a tie that would make a circle of callouts is refused.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellEdit, Document, FormulaSlot, MAX_SHAPES_PER_PAGE, Owner, RowKey, SectionKey, Shape, ShapeRef, ShapeType, Sheet, SheetRef,
    Value,
};

use super::read::Reader;
use super::route::P;
use super::scan::{scan, scan_every};
use super::{Problem, checked, page_mut, require_page, require_shape, write_cells};
use crate::stencil::{CALLOUT_HEIGHT, CALLOUT_WIDTH, CalloutFrame, CalloutStyle, callout_shape, free_tip_cells, tied_tip_cells};

/// The most callouts one shape has.
pub const MAX_CALLOUTS_PER_SHAPE: usize = 256;

/// The most callouts that follow one edit (a move, a place), the callouts of callouts counted.
pub const MAX_FOLLOWING: usize = 20_000;

/// Most rounds of finding the callouts of callouts, and most links of a chain of callouts walked for a circle.
const MAX_ROUNDS: usize = 16;
const MAX_CHAIN: usize = 64;

pub use super::ties::MAX_TIE_MARKS;
use super::ties::{CALLOUTS, LIST_MEMBERS, MEMBERS, TARGET, Ties};
#[cfg(test)]
use super::ties::{MARKS_SCANNED, TIES_READ};

/// The text of a `User` row's `Value`, held by the shape itself.
fn user_text(shape: &Shape, row: &str) -> Option<String> {
    let section = shape.sheet.section(&SectionKey::new("User", None))?;
    let row = section.row(&RowKey::name(row))?;
    row.cells.get("Value")?.value.as_ref().map(|v| v.text().to_owned())
}

/// Whether the shape says it is a callout: `User.msvStructureType`, or a tie to a shape it points at.
fn is_callout(shape: &Shape) -> bool {
    user_text(shape, "msvStructureType").is_some_and(|t| t.trim() == "Callout")
        || Ties::of_shape(shape).is_some_and(|t| !t.targets(TARGET).is_empty())
}

/// The gallery tile's id a callout DiaCraft made holds, and so whether it holds a tip DiaCraft can move.
fn style_of(shape: &Shape) -> Option<String> {
    user_text(shape, "CalloutStyle")
}

fn tip_addr(cell: &str) -> CellAddr {
    CellAddr::in_row("Scratch", None, RowKey::Ix(0), cell)
}

// ---- what is asked about a callout -------------------------------------------------------------------------------------------

/// What a shape is to callouts.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The callouts tied to the shape.
    pub callouts: Vec<u32>,
    /// The shape the callout points at.
    pub target: Option<u32>,
    /// Whether the shape is a callout.
    pub is_callout: bool,
    /// The gallery tile's id of a callout DiaCraft made.
    pub style: Option<String>,
    /// Where the end of the leader is on the page, in drawing units, for a callout DiaCraft made.
    pub tip: Option<P>,
}

/// What shape `id` of a page is to callouts.
pub fn info(doc: &Document, page: u32, id: u32) -> Result<Info, Problem> {
    let at = require_shape(doc, page, id)?;
    let shape = doc.shape(&at).ok_or_else(|| Problem::failed("the shape went away"))?;
    let reader = Reader::new(doc);
    let sheet = SheetRef::Shape(at.clone());
    let ties = Ties::of_sheet(&reader, &sheet);
    let style = style_of(shape);
    let tip = style.as_ref().map(|_| reader.to_page(&at, (reader.addr_number(&sheet, &tip_addr("X")), reader.addr_number(&sheet, &tip_addr("Y")))));
    Ok(Info { callouts: ties.targets(CALLOUTS), target: ties.targets(TARGET).first().copied(), is_callout: is_callout(shape), style, tip })
}

// ---- tying and untying ------------------------------------------------------------------------------------------------------

/// Whether a callout may be tied to a target, or why not, in words.
fn check_tie(doc: &Document, page: u32, callout: u32, target: u32) -> Result<(ShapeRef, ShapeRef), Problem> {
    if callout == target {
        return Err(Problem::bad(format!("shape {callout} cannot point at itself")));
    }
    let c_at = require_shape(doc, page, callout)?;
    let t_at = require_shape(doc, page, target)?;
    let c_shape = doc.shape(&c_at).ok_or_else(|| Problem::failed("the shape went away"))?;
    let t_shape = doc.shape(&t_at).ok_or_else(|| Problem::failed("the shape went away"))?;
    if !is_callout(c_shape) {
        return Err(Problem::bad(format!("shape {callout} is not a callout (its User.msvStructureType is not Callout)")));
    }
    if c_at.path.len() != 1 {
        return Err(Problem::bad(format!(
            "callout {callout} is inside a group: a callout is tied to a shape from the page's top level (take it out of the group)"
        )));
    }
    if t_at.path.len() != 1 {
        return Err(Problem::bad(format!(
            "shape {target} is inside a group: a callout points at a shape of the page's top level, so tie it to the group"
        )));
    }
    if t_shape.kind == ShapeType::Guide {
        return Err(Problem::bad(format!("shape {target} is a guide: a callout cannot point at one")));
    }
    // A circle of callouts, each reading the pin of the other, would be a loop in the recalculation: walk up from the target through the shapes it points at.
    let mut at = target;
    let mut seen: HashSet<u32> = HashSet::new();
    for _ in 0..MAX_CHAIN {
        if at == callout {
            return Err(Problem::bad(format!(
                "shape {target} is a callout of callout {callout} (or points at it through other callouts): the tie would make a circle"
            )));
        }
        if !seen.insert(at) {
            break;
        }
        let Some(next) = doc
            .find_shape(Owner::Page(page), at)
            .and_then(|r| doc.shape(&r))
            .and_then(|s| Ties::of_shape(s))
            .and_then(|t| t.targets(TARGET).first().copied())
        else {
            break;
        };
        at = next;
    }
    Ok((c_at, t_at))
}

/// The writes that tie `callout` to `target`: the callout's `Relationships` points at the target and the target's names the callout, a callout the target was tied to before loses it, and a callout DiaCraft
/// made has formulas for the end of its leader.
fn tie_writes(doc: &Document, page: u32, callout: u32, target: u32) -> Result<Vec<(SheetRef, CellAddr, CellEdit)>, Problem> {
    let (c_at, t_at) = check_tie(doc, page, callout, target)?;
    tie_writes_for(doc, page, (callout, &c_at), (target, &t_at))
}

/// [`tie_writes`] for a callout and a target that are found already, and checked: a caller that makes many callouts finds their targets in one walk of the page and knows the places of the callouts it has
/// just put at the end of it, so that no tie searches the page.
fn tie_writes_for(
    doc: &Document,
    page: u32,
    (callout, c_at): (u32, &ShapeRef),
    (target, t_at): (u32, &ShapeRef),
) -> Result<Vec<(SheetRef, CellAddr, CellEdit)>, Problem> {
    let reader = Reader::new(doc);
    let (c_sheet, t_sheet) = (SheetRef::Shape(c_at.clone()), SheetRef::Shape(t_at.clone()));
    let mut c_ties = Ties::of_sheet(&reader, &c_sheet);
    let mut t_ties = Ties::of_sheet(&reader, &t_sheet);
    if !t_ties.targets(CALLOUTS).contains(&callout) && t_ties.count(CALLOUTS) >= MAX_CALLOUTS_PER_SHAPE {
        return Err(Problem::failed(format!("a shape has at most {MAX_CALLOUTS_PER_SHAPE} callouts")));
    }
    c_ties.check_rewritable(callout)?;
    t_ties.check_rewritable(target)?;
    let before = c_ties.targets(TARGET);
    c_ties.clear(TARGET);
    c_ties.add(TARGET, target);
    t_ties.add(CALLOUTS, callout);
    let mut writes = vec![c_ties.write(c_sheet.clone()), t_ties.write(t_sheet)];
    let others: Vec<u32> = before.into_iter().filter(|old| *old != target).collect();
    if !others.is_empty() {
        let wanted: HashSet<u32> = others.iter().copied().collect();
        let found = scan(doc, page, &wanted)?;
        for old in others {
            if let Some(at) = found.found.get(&old) {
                let sheet = SheetRef::Shape(at.clone());
                let mut ties = Ties::of_sheet(&reader, &sheet);
                if ties.remove(CALLOUTS, callout) {
                    ties.check_rewritable(old)?;
                    writes.push(ties.write(sheet));
                }
            }
        }
    }
    if doc.shape(c_at).is_some_and(|s| style_of(s).is_some()) {
        for (name, cell) in tied_tip_cells() {
            writes.push((c_sheet.clone(), tip_addr(name), CellEdit::Cell(cell)));
        }
    }
    Ok(writes)
}

/// Ties callout `callout` to the shape `target`. The caller recalculates the page (`settle`).
pub fn tie(doc: &mut Document, page: u32, callout: u32, target: u32) -> Result<(), Problem> {
    let writes = tie_writes(doc, page, callout, target)?;
    write_cells(doc, writes)
}

/// Ties a callout to nothing. The end of its leader is where it is now in the callout's own frame, or at (`x`, `y`) on the page when they are given (the page's drawing units). → whether anything changed:
/// a callout tied to nothing, and given no place, is as it was.
pub fn untie(doc: &mut Document, page: u32, callout: u32, at: Option<P>) -> Result<bool, Problem> {
    let c_at = require_shape(doc, page, callout)?;
    let c_shape = doc.shape(&c_at).ok_or_else(|| Problem::failed("the shape went away"))?;
    if !is_callout(c_shape) {
        return Err(Problem::bad(format!("shape {callout} is not a callout (its User.msvStructureType is not Callout)")));
    }
    if let Some((x, y)) = at {
        checked("x", x)?;
        checked("y", y)?;
        if at.is_some() && c_at.path.len() != 1 {
            return Err(Problem::bad(format!("callout {callout} is inside a group: its end is put at a place on the page only from the top level")));
        }
    }
    let ours = style_of(c_shape).is_some();
    if at.is_some() && !ours {
        return Err(Problem::bad(format!("callout {callout} is not one DiaCraft made: it has no end of its leader to put at a place")));
    }
    let reader = Reader::new(doc);
    let c_sheet = SheetRef::Shape(c_at.clone());
    let mut c_ties = Ties::of_sheet(&reader, &c_sheet);
    let targets = c_ties.targets(TARGET);
    if targets.is_empty() && at.is_none() {
        return Ok(false);
    }
    c_ties.check_rewritable(callout)?;
    c_ties.clear(TARGET);
    let mut writes = Vec::new();
    if !targets.is_empty() || !c_ties.calls.is_empty() {
        writes.push(c_ties.write(c_sheet.clone()));
    }
    if ours {
        let (x, y) = match at {
            Some(place) => reader.pose(&c_sheet).down(place),
            None => (reader.addr_number(&c_sheet, &tip_addr("X")), reader.addr_number(&c_sheet, &tip_addr("Y"))),
        };
        for (name, cell) in free_tip_cells((x, y)) {
            writes.push((c_sheet.clone(), tip_addr(name), CellEdit::Cell(cell)));
        }
    }
    let wanted: HashSet<u32> = targets.iter().copied().collect();
    let found = scan(doc, page, &wanted)?;
    for old in targets {
        if let Some(t_at) = found.found.get(&old) {
            let sheet = SheetRef::Shape(t_at.clone());
            let mut ties = Ties::of_sheet(&reader, &sheet);
            if ties.remove(CALLOUTS, callout) {
                ties.check_rewritable(old)?;
                writes.push(ties.write(sheet));
            }
        }
    }
    write_cells(doc, writes)?;
    Ok(true)
}

// ---- adding callouts --------------------------------------------------------------------------------------------------------

/// A callout made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Added {
    /// The new shape's `ID`.
    pub id: u32,
    /// The shape it is tied to.
    pub target: Option<u32>,
}

/// The gap between a target's box and the callout beside it, in inches.
const GAP: f64 = 0.3;

/// Adds one callout of `style` for each shape of `targets`, or one tied to nothing at `at` when there are none (the page's drawing units; the middle of the page when `at` is `None`). A callout for a shape
/// is up and to the right of it, or below or to the left when the page has no room there, with its leader's end at the shape's edge nearest it. Their `ID`s are above `above` as well as above every `ID` on
/// the page (the session's high-water mark). The caller puts them on the page's theme and layers and recalculates.
pub fn add_callouts(
    doc: &mut Document,
    page: u32,
    style: &CalloutStyle,
    targets: &[u32],
    at: Option<P>,
    text: &str,
    above: u32,
) -> Result<Vec<Added>, Problem> {
    require_page(doc, page)?;
    super::text_edit::check_text(text)?;
    if let Some((x, y)) = at {
        checked("x", x)?;
        checked("y", y)?;
    }
    let (highest, count) = super::shapes::page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    let wanted = targets.len().max(1);
    if count.saturating_add(wanted) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let k = super::units_per_inch(doc, page);
    let (width, height) = (CALLOUT_WIDTH * k, CALLOUT_HEIGHT * k);
    // Where each callout goes, and which shape each points at, found before anything is added.
    let mut plans: Vec<(P, Option<u32>)> = Vec::with_capacity(wanted);
    let mut target_ats: Vec<ShapeRef> = Vec::with_capacity(wanted);
    {
        let reader = Reader::new(doc);
        let sheet = SheetRef::Page(page);
        let (page_w, page_h) = (reader.number(&sheet, "PageWidth"), reader.number(&sheet, "PageHeight"));
        if targets.is_empty() {
            let (x, y) = at.unwrap_or((page_w * 0.5, page_h * 0.5));
            plans.push(((x, y), None));
        } else {
            let mut seen: HashSet<u32> = HashSet::new();
            let unique: Vec<u32> = targets.iter().copied().filter(|id| seen.insert(*id)).collect();
            // The targets are found by one walk of the page, not a search of it for each.
            let found: HashMap<u32, ShapeRef> = found_in(doc, page, &unique)?.into_iter().collect();
            for id in unique {
                let t_at = found.get(&id).cloned().ok_or_else(|| Problem::bad(format!("page {page} has no shape {id}")))?;
                target_ats.push(t_at.clone());
                let shape = doc.shape(&t_at).ok_or_else(|| Problem::failed("the shape went away"))?;
                if t_at.path.len() != 1 {
                    return Err(Problem::bad(format!(
                        "shape {id} is inside a group: a callout points at a shape of the page's top level, so tie it to the group"
                    )));
                }
                if shape.kind == ShapeType::Guide {
                    return Err(Problem::bad(format!("shape {id} is a guide: a callout cannot point at one")));
                }
                let t_sheet = SheetRef::Shape(t_at.clone());
                let held = Ties::of_sheet(&reader, &t_sheet).count(CALLOUTS);
                if held >= MAX_CALLOUTS_PER_SHAPE {
                    return Err(Problem::failed(format!("shape {id} has {held} callouts, the most a shape has")));
                }
                let corners = reader.corners(&t_at);
                let (mut lo, mut hi) = ((f64::INFINITY, f64::INFINITY), (f64::NEG_INFINITY, f64::NEG_INFINITY));
                for (x, y) in corners {
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                }
                if !(lo.0.is_finite() && lo.1.is_finite() && hi.0.is_finite() && hi.1.is_finite()) {
                    return Err(Problem::failed(format!("shape {id} has no box on the page")));
                }
                let gap = GAP * k;
                // Up and to the right of the shape, or to its left when the leader leaves by the right side of the callout; the other way when the page has no room.
                let (right_of, left_of) = (hi.0 + gap * 0.5 + width * 0.5, lo.0 - gap * 0.5 - width * 0.5);
                let (mut cx, other) = if style.faces_right() { (left_of, right_of) } else { (right_of, left_of) };
                let mut cy = hi.1 + gap + height * 0.5;
                if page_h > 0.0 && cy + height * 0.5 > page_h {
                    cy = lo.1 - gap - height * 0.5;
                }
                if page_w > 0.0
                    && (cx + width * 0.5 > page_w || cx - width * 0.5 < 0.0)
                    && other - width * 0.5 >= 0.0
                    && other + width * 0.5 <= page_w
                {
                    cx = other;
                }
                // A shape that has callouts already gets the next one above the last.
                cy += f64::from(u32::try_from(held).unwrap_or(u32::MAX)) * (height + gap * 0.3);
                plans.push(((checked("x", cx)?, checked("y", cy)?), Some(id)));
            }
        }
    }
    let first = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    // The callouts go at the end of the page's list, so the place of each is known.
    let base = doc.page(page).map_or(0, |p| p.shapes.len());
    let mut added: Vec<Added> = Vec::with_capacity(plans.len());
    for (k_id, (pin, target)) in plans.iter().enumerate() {
        let id = u32::try_from(k_id).ok().and_then(|n| first.checked_add(n)).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        let frame = CalloutFrame { pin: *pin, width, height, tip: style.default_tip(width, k) };
        let shape = callout_shape(id, style, &frame, text);
        page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.push_shape(Arc::new(shape));
        added.push(Added { id, target: *target });
    }
    // The ties are made with the shapes in the page, so that the formulas find their sheets.
    let mut writes = Vec::new();
    for (k_at, a) in added.iter().enumerate() {
        if let (Some(target), Some(t_at)) = (a.target, target_ats.get(k_at)) {
            let place = u32::try_from(base.saturating_add(k_at)).map_err(|_| Problem::failed("a page holds too many shapes"))?;
            let c_at = ShapeRef { owner: Owner::Page(page), path: vec![place] };
            writes.extend(tie_writes_for(doc, page, (a.id, &c_at), (target, t_at))?);
        }
    }
    write_cells(doc, writes)?;
    Ok(added)
}

// ---- what follows a move ----------------------------------------------------------------------------------------------------

/// Calls `f` with the shape and each shape inside it. Iterative; `visits` is what the call may still look at, and the walk is refused in words past it.
fn each_in<'a>(shape: &'a Shape, visits: &mut usize, mut f: impl FnMut(&'a Shape) -> Result<(), Problem>) -> Result<(), Problem> {
    let mut stack: Vec<&'a Shape> = vec![shape];
    while let Some(current) = stack.pop() {
        if *visits == 0 {
            return Err(Problem::failed(format!("the callouts of the shapes cannot be found: they reach more than {MAX_SHAPES_PER_PAGE} shapes")));
        }
        *visits -= 1;
        f(current)?;
        stack.extend(current.children.iter().rev().map(|c| &**c));
    }
    Ok(())
}

/// The `ID`s of the shape and of everything inside it, and the callouts that the `Relationships` of each of them names.
fn subtree_callouts(shape: &Shape, visits: &mut usize, ids: &mut HashSet<u32>, callouts: &mut Vec<u32>) -> Result<(), Problem> {
    subtree_links(shape, visits, ids, callouts, callouts_named)
}

/// The `ID`s of the shape and of everything inside it, and what `link` says goes along with each of them.
fn subtree_links(
    shape: &Shape,
    visits: &mut usize,
    ids: &mut HashSet<u32>,
    out: &mut Vec<u32>,
    link: fn(u32, &Ties) -> Result<Vec<u32>, Problem>,
) -> Result<(), Problem> {
    each_in(shape, visits, |s| {
        ids.insert(s.id);
        if let Some(t) = Ties::of_shape(s) {
            out.extend(link(s.id, &t)?);
        }
        Ok(())
    })
}

/// The callouts a shape's ties name. A shape has at most [`MAX_CALLOUTS_PER_SHAPE`], and a file that names more is refused in words when its shape is moved, copied or deleted: each callout is another shape to
/// find on the page.
fn callouts_named(id: u32, ties: &Ties) -> Result<Vec<u32>, Problem> {
    ties.check_rewritable(id)?;
    let named = ties.targets(CALLOUTS);
    if named.len() > MAX_CALLOUTS_PER_SHAPE {
        return Err(Problem::failed(format!(
            "shape {id} names {} callouts, more than the {MAX_CALLOUTS_PER_SHAPE} a shape has: take some of them away (callout.detach) before moving, copying or deleting it",
            named.len()
        )));
    }
    Ok(named)
}

/// What goes along with a shape that is moved: its callouts and, when it is a container, its members (**one walk for both**, so a callout tied to a member follows the container too and no
/// shape is looked at twice). The members are those of a plain list of ties ([`Ties::lists_members`]: a formula with anything else in it is not read as a container's), at most what a formula is read as
/// naming ([`MAX_TIE_MARKS`]), and all that follow one edit are bounded by [`MAX_FOLLOWING`].
fn follows(id: u32, ties: &Ties) -> Result<Vec<u32>, Problem> {
    let mut named = callouts_named(id, ties)?;
    if ties.holds_parts() {
        // A container's members, and a list's (a frame that moves takes its lists, and a list takes its lanes).
        named.extend(ties.ids(MEMBERS).into_iter().chain(ties.ids(LIST_MEMBERS)).filter(|m| *m != id));
    }
    Ok(named)
}

/// The shapes that go along with `shape` when it is moved: its callouts and, if it is a container, its members, as [`followers`] finds them, for one shape and with no walk of the page.
/// A layout asks it of every shape it would move, to leave out the ones that move with another (`relayout/scope.rs`).
pub(crate) fn follow_of(shape: &Shape) -> Result<Vec<u32>, Problem> {
    match Ties::of_shape(shape) {
        Some(ties) => follows(shape.id, &ties),
        None => Ok(Vec::new()),
    }
}

/// The shapes of `found` (each with its place), the outermost only, in document order: a shape inside another of them is reached from it. A shape's members follow it directly in document order, so the last
/// shape kept is the only one that can hold the next.
fn outermost_of(mut found: Vec<(u32, ShapeRef)>) -> Vec<(u32, ShapeRef)> {
    found.sort_by(|a, b| a.1.path.cmp(&b.1.path));
    let mut kept: Vec<(u32, ShapeRef)> = Vec::with_capacity(found.len());
    for (id, at) in found {
        if kept.last().is_some_and(|(_, last)| at.path.len() > last.path.len() && at.path.starts_with(&last.path)) {
            continue;
        }
        kept.push((id, at));
    }
    kept
}

/// Where the shapes `ids` are on a page: a search for each of a few, one walk of the page for more (a search walks the page from its top, so many of them would cost the product).
fn found_in(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<(u32, ShapeRef)>, Problem> {
    if ids.len() <= 8 {
        return Ok(ids.iter().filter_map(|id| doc.find_shape(Owner::Page(page), *id).map(|at| (*id, at))).collect());
    }
    let wanted: HashSet<u32> = ids.iter().copied().collect();
    Ok(scan(doc, page, &wanted)?.found.into_iter().collect())
}

/// The callouts that follow the shapes moved, each with where it is and the distance of the shape it follows (the page's drawing units): the callouts tied to a shape moved or to a shape inside one, and
/// theirs, but not a callout that moves already (named, or inside a shape that is). `moved` are the shapes named, **each with where it is**, which the caller has found already: a callout is found with the
/// one walk of the page that every round of callouts costs, never by a search from the top of the page for each (a shape a file ties thousands of callouts to would cost a walk each). A shape inside a
/// shape moved that is moved itself is carried by both, and its callouts go by the sum. Empty, with no walk of the page, when none of the shapes holds a tie to a callout. Each shape is looked at once,
/// whatever the nesting.
pub(crate) fn followers(doc: &Document, page: u32, moved: &[(u32, ShapeRef, P)]) -> Result<Vec<(u32, ShapeRef, P)>, Problem> {
    followers_leaving(doc, page, moved, &HashSet::new())
}

/// [`followers`] that passes over the shapes `left` (a move leaves them where they are, `shapes::move_shapes_leaving`): none of them is among the followers, and
/// what follows them is not followed through them (it follows still when another shape that moves leads to it).
pub(crate) fn followers_leaving(
    doc: &Document,
    page: u32,
    moved: &[(u32, ShapeRef, P)],
    left: &HashSet<u32>,
) -> Result<Vec<(u32, ShapeRef, P)>, Problem> {
    // A shape named twice moves twice.
    let mut named: HashMap<u32, P> = HashMap::with_capacity(moved.len());
    for (id, _, d) in moved {
        let total = named.entry(*id).or_insert((0.0, 0.0));
        *total = (total.0 + d.0, total.1 + d.1);
    }
    let mut first: HashSet<u32> = HashSet::with_capacity(moved.len());
    let roots = outermost_of(moved.iter().filter(|(id, _, _)| first.insert(*id)).map(|(id, at, _)| (*id, at.clone())).collect());
    let mut visits = MAX_SHAPES_PER_PAGE;
    let mut inside: HashSet<u32> = HashSet::new();
    let mut wanted: Vec<(u32, P)> = Vec::new();
    let mut queued: HashSet<u32> = HashSet::new();
    for (_, at) in &roots {
        let Some(root) = doc.shape(at) else { continue };
        // Depth first, each shape with the distance it has gone: the sum of the shapes moved on the way down to it and itself.
        let mut stack: Vec<(&Shape, P)> = vec![(root, (0.0, 0.0))];
        while let Some((shape, carried)) = stack.pop() {
            if visits == 0 {
                return Err(Problem::failed(format!(
                    "the callouts of the shapes cannot be found: they reach more than {MAX_SHAPES_PER_PAGE} shapes"
                )));
            }
            visits -= 1;
            inside.insert(shape.id);
            let own = named.get(&shape.id).copied().unwrap_or((0.0, 0.0));
            let gone = (carried.0 + own.0, carried.1 + own.1);
            if let Some(ties) = Ties::of_shape(shape) {
                // A shape that a container holds, or a callout is tied to, is asked for once, whatever the number of the shapes that name it.
                wanted.extend(follows(shape.id, &ties)?.into_iter().filter(|c| queued.insert(*c)).map(|c| (c, gone)));
            }
            stack.extend(shape.children.iter().rev().map(|c| (&**c, gone)));
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<(u32, ShapeRef, P)> = Vec::new();
    let mut seen: HashSet<u32> = inside;
    seen.extend(named.keys().copied());
    // Every shape of the page by its `ID`, made once, when a second round is needed (a container in a container, a callout of a callout): a walk of the page for each round would cost the depth of the nesting
    // times the page. Each round adds a shape not seen before, so the rounds end within [`MAX_FOLLOWING`].
    let mut every: Option<HashMap<u32, ShapeRef>> = None;
    for round in 0..=MAX_FOLLOWING {
        wanted.retain(|(c, _)| seen.insert(*c));
        if wanted.is_empty() {
            break;
        }
        if out.len().saturating_add(wanted.len()) > MAX_FOLLOWING {
            return Err(Problem::failed(format!("more than {MAX_FOLLOWING} callouts and members would follow the shapes moved")));
        }
        let first;
        let found: &HashMap<u32, ShapeRef> = if round == 0 {
            let ids: HashSet<u32> = wanted.iter().map(|(c, _)| *c).collect();
            first = scan(doc, page, &ids)?.found;
            &first
        } else {
            if every.is_none() {
                every = Some(scan_every(doc, page)?);
            }
            let Some(all) = every.as_ref() else { break };
            all
        };
        let mut next: Vec<(u32, P)> = Vec::new();
        for (c, d) in wanted {
            if left.contains(&c) {
                continue;
            }
            let Some(at) = found.get(&c) else { continue };
            let Some(shape) = doc.shape(at) else { continue };
            out.push((c, at.clone(), d));
            // What follows a callout or a member follows too: its callouts, and its members if it is a container.
            let (mut more, mut ignore) = (Vec::new(), HashSet::new());
            subtree_links(shape, &mut visits, &mut ignore, &mut more, follows)?;
            next.extend(more.into_iter().filter(|m| queued.insert(*m)).map(|m| (m, d)));
        }
        wanted = next;
    }
    // A shape on a locked layer is not moved by what it follows either: the move is refused in words that name the layer.
    let moving = out.iter().filter(|(_, _, d)| d.0 != 0.0 || d.1 != 0.0).filter_map(|(id, at, _)| doc.shape(at).map(|s| (*id, &**s)));
    super::layers::refuse_locked_followers(doc, page, moving, "moved")?;
    Ok(out)
}

// ---- what a delete takes ---------------------------------------------------------------------------------------------------

/// What deleting shapes takes besides them.
#[derive(Debug, Default)]
pub(crate) struct DeleteExtra {
    /// The paths of the callouts that go with them.
    pub paths: Vec<Vec<u32>>,
    /// The `ID`s of those and of everything inside them.
    pub gone: BTreeSet<u32>,
    /// The writes that take a callout that goes out of the `Relationships` of the shape it points at, which stays.
    pub writes: Vec<(SheetRef, CellAddr, CellEdit)>,
}

/// The paths of `paths` that are not inside another of them, in document order.
fn outermost_paths(paths: impl Iterator<Item = Vec<u32>>) -> Vec<Vec<u32>> {
    let mut all: Vec<Vec<u32>> = paths.collect();
    all.sort();
    all.dedup();
    let mut kept: Vec<Vec<u32>> = Vec::with_capacity(all.len());
    for path in all {
        if kept.last().is_some_and(|last| path.len() > last.len() && path.starts_with(last)) {
            continue;
        }
        kept.push(path);
    }
    kept
}

/// The callouts that a delete of the shapes at `paths` (whose `ID`s, and those of everything inside them, are `gone`) takes along, and what the shapes that stay need to forget.
pub(crate) fn delete_extra(doc: &Document, page: u32, paths: &[Vec<u32>], gone: &BTreeSet<u32>) -> Result<DeleteExtra, Problem> {
    let mut extra = DeleteExtra::default();
    let mut visits = MAX_SHAPES_PER_PAGE;
    let mut now: HashSet<u32> = gone.iter().copied().collect();
    let shape_at = |path: &Vec<u32>| doc.shape(&ShapeRef { owner: Owner::Page(page), path: path.clone() });
    // The callouts of the shapes that go, then theirs. A shape named inside another that is named is reached from it: each is looked at once.
    let roots = outermost_paths(paths.iter().cloned());
    let mut frontier: Vec<u32> = Vec::new();
    for path in &roots {
        if let Some(shape) = shape_at(path) {
            subtree_callouts(shape, &mut visits, &mut HashSet::new(), &mut frontier)?;
        }
    }
    let mut queued: HashSet<u32> = HashSet::new();
    for _ in 0..MAX_ROUNDS {
        frontier.retain(|c| !now.contains(c) && queued.insert(*c));
        if frontier.is_empty() {
            break;
        }
        if extra.paths.len().saturating_add(frontier.len()) > MAX_FOLLOWING {
            return Err(Problem::failed(format!("more than {MAX_FOLLOWING} callouts would go with the shapes deleted")));
        }
        let wanted: HashSet<u32> = frontier.iter().copied().collect();
        let found = scan(doc, page, &wanted)?;
        let mut next: Vec<u32> = Vec::new();
        for id in std::mem::take(&mut frontier) {
            let Some(at) = found.found.get(&id) else { continue };
            let Some(shape) = doc.shape(at) else { continue };
            extra.paths.push(at.path.clone());
            let mut inner = HashSet::new();
            subtree_callouts(shape, &mut visits, &mut inner, &mut next)?;
            now.extend(inner);
        }
        frontier = next;
    }
    extra.gone = now.iter().copied().filter(|id| !gone.contains(id)).collect();
    // Each callout that goes points at a shape that may stay: that shape forgets it.
    let mut pointed: HashMap<u32, Vec<u32>> = HashMap::new();
    for path in outermost_paths(paths.iter().cloned().chain(extra.paths.iter().cloned())) {
        let Some(shape) = shape_at(&path) else { continue };
        each_in(shape, &mut visits, |s| {
            if let Some(ties) = Ties::of_shape(s) {
                for target in ties.targets(TARGET) {
                    if !now.contains(&target) {
                        pointed.entry(target).or_default().push(s.id);
                    }
                }
            }
            Ok(())
        })?;
    }
    if !pointed.is_empty() {
        let wanted: HashSet<u32> = pointed.keys().copied().collect();
        let found = scan(doc, page, &wanted)?;
        for (target, going) in pointed {
            let Some(at) = found.found.get(&target) else { continue };
            let Some(shape) = doc.shape(at) else { continue };
            let Some(mut ties) = Ties::of_shape(shape) else { continue };
            // One pass over the marks takes out every callout that goes, whatever their number.
            let going: HashSet<u32> = going.into_iter().collect();
            if ties.retain_sheets(CALLOUTS, |id| !going.contains(&id)) {
                ties.check_rewritable(shape.id)?;
                extra.writes.push(ties.write(SheetRef::Shape(at.clone())));
            }
        }
    }
    Ok(extra)
}

// ---- what a copy takes ------------------------------------------------------------------------------------------------------

/// The shapes to copy, with the callouts tied to them: the callouts of the shapes named, and of those, that are on the page's top level and not named, in document order.
pub(crate) fn with_callouts(doc: &Document, page: u32, shapes: Vec<(u32, ShapeRef)>) -> Result<Vec<(u32, ShapeRef)>, Problem> {
    let mut visits = MAX_SHAPES_PER_PAGE;
    let mut have: HashSet<u32> = shapes.iter().map(|(id, _)| *id).collect();
    let mut wanted: Vec<u32> = Vec::new();
    for (_, at) in &shapes {
        let Some(shape) = doc.shape(at) else { continue };
        subtree_callouts(shape, &mut visits, &mut HashSet::new(), &mut wanted)?;
    }
    if wanted.is_empty() {
        return Ok(shapes);
    }
    let mut out = shapes;
    for _ in 0..MAX_ROUNDS {
        wanted.retain(|c| have.insert(*c));
        if wanted.is_empty() {
            break;
        }
        if out.len().saturating_add(wanted.len()) > MAX_FOLLOWING {
            return Err(Problem::failed(format!("more than {MAX_FOLLOWING} callouts would be copied with the shapes")));
        }
        let ids: HashSet<u32> = wanted.iter().copied().collect();
        let found = scan(doc, page, &ids)?;
        let mut next = Vec::new();
        for id in wanted {
            let Some(at) = found.found.get(&id) else { continue };
            // A callout inside a group is not copied by itself: it goes with the group, or not at all.
            if at.path.len() != 1 {
                continue;
            }
            let Some(shape) = doc.shape(at) else { continue };
            out.push((id, at.clone()));
            subtree_callouts(shape, &mut visits, &mut HashSet::new(), &mut next)?;
        }
        wanted = next;
    }
    out.sort_by(|a, b| a.1.path.cmp(&b.1.path));
    Ok(out)
}

/// The sheet of a copy of a shape whose copy of the shapes in `copied` (by `ID`) is made: the ties to callouts that are not copied are taken out of it, and a callout whose target is not copied is tied to
/// nothing, its leader's end where it was in its own frame. The sheet itself when it has no such tie. One pass over the marks of each call, whatever their number; a formula that
/// names more than [`MAX_TIE_MARKS`] shapes is refused in words (the copy would have to drop the rest).
pub(crate) fn cut_uncopied(id: u32, sheet: &Arc<Sheet>, copied: &HashMap<u32, u32>) -> Result<Arc<Sheet>, Problem> {
    let Some(cell) = sheet.cell("Relationships") else { return Ok(Arc::clone(sheet)) };
    let FormulaSlot::Local(formula) = &cell.formula else { return Ok(Arc::clone(sheet)) };
    let Ok(root) = &formula.ast else { return Ok(Arc::clone(sheet)) };
    let mut ties = Ties::from_expr(root);
    ties.check_rewritable(id)?;
    let mut changed = false;
    let mut freed = false;
    for code in [CALLOUTS, TARGET] {
        let any = ties.retain_sheets(code, |named| copied.contains_key(&named));
        changed |= any;
        freed |= any && code == TARGET;
    }
    if !changed {
        return Ok(Arc::clone(sheet));
    }
    let mut next = (**sheet).clone();
    if next.set_cell(&CellAddr::direct("Relationships"), Cell::with_formula(&ties.print(), Some(Value::from_text("0")))).is_err() {
        return Ok(Arc::clone(sheet));
    }
    if freed {
        let held = |name: &str| {
            let section = sheet.section(&SectionKey::new("Scratch", None))?;
            section.row(&RowKey::Ix(0))?.cells.get(name)?.value.as_ref()?.number()
        };
        if let (Some(x), Some(y)) = (held("X"), held("Y")) {
            for (name, cell) in free_tip_cells((x, y)) {
                if next.set_cell(&tip_addr(name), cell).is_err() {
                    return Ok(Arc::clone(sheet));
                }
            }
        }
    }
    Ok(Arc::new(next))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn ties_of(formula: &str) -> Ties {
        let parsed = diacraft_formula::parse(formula);
        Ties::from_expr(parsed.ast.as_ref().unwrap())
    }

    #[test]
    fn the_calls_of_a_relationships_formula_are_read_in_order_with_their_shapes() {
        let ties = ties_of("SUM(DEPENDSON(3,Sheet.7!SheetRef(),Sheet.9!SheetRef()),DEPENDSON(4),DEPENDSON(6,REF()))");
        assert_eq!(ties.targets(3), [7, 9]);
        assert!(ties.targets(4).is_empty());
        assert!(ties.targets(6).is_empty(), "a lost target is no target");
        assert_eq!(ties.calls.len(), 3);
    }

    #[test]
    fn a_tie_is_added_removed_and_printed_as_the_files_write_it() {
        let mut ties = ties_of("SUM(DEPENDSON(1,Sheet.2!SheetRef()),DEPENDSON(3,Sheet.7!SheetRef()))");
        ties.add(3, 8);
        ties.add(3, 8);
        assert_eq!(ties.print(), "SUM(DEPENDSON(1,Sheet.2!SheetRef()),DEPENDSON(3,Sheet.7!SheetRef(),Sheet.8!SheetRef()))");
        assert!(ties.remove(3, 7));
        assert!(!ties.remove(3, 7));
        ties.add(6, 4);
        assert_eq!(ties.print(), "SUM(DEPENDSON(1,Sheet.2!SheetRef()),DEPENDSON(3,Sheet.8!SheetRef()),DEPENDSON(6,Sheet.4!SheetRef()))");
        ties.clear(3);
        assert_eq!(ties.print(), "SUM(DEPENDSON(1,Sheet.2!SheetRef()),DEPENDSON(3),DEPENDSON(6,Sheet.4!SheetRef()))");
    }

    #[test]
    fn a_target_that_is_not_a_sheet_id_and_a_lost_one_are_kept_as_they_were() {
        let mut ties = ties_of("SUM(DEPENDSON(3,Door!SheetRef(),REF(),Sheet.7!SheetRef()))");
        ties.add(3, 9);
        assert_eq!(ties.print(), "SUM(DEPENDSON(3,Door!SheetRef(),REF(),Sheet.7!SheetRef(),Sheet.9!SheetRef()))");
        assert_eq!(ties.targets(3), [7, 9]);
    }

    // ---- what a file can make a move, a copy and a delete cost (counted, not timed) ----------------------------------------------

    fn boxed(id: u32, x: f64, y: f64) -> Shape {
        let mut shape = Shape::new(id);
        let mut sheet = Sheet::default();
        for (name, value) in [("PinX", x), ("PinY", y), ("Width", 1.0), ("Height", 1.0), ("LocPinX", 0.0), ("LocPinY", 0.0)] {
            sheet.cells.insert(diacraft_doc::CellName::new(name), Cell::constant(Value::from_text(&value.to_string())).with_unit("IN"));
        }
        shape.sheet = Arc::new(sheet);
        shape
    }

    fn tie_cell(shape: &mut Shape, formula: &str) {
        let sheet = Arc::make_mut(&mut shape.sheet);
        sheet.cells.insert(diacraft_doc::CellName::new("Relationships"), Cell::with_formula(formula, Some(Value::from_text("0"))));
    }

    fn doc_of(shapes: Vec<Shape>) -> Document {
        let mut doc = Document::new_blank();
        super::page_mut(&mut doc, 1).unwrap().shapes = diacraft_doc::ShapeList::from_vec(shapes.into_iter().map(Arc::new).collect());
        doc
    }

    /// `n` targets (1 to n), each with a callout (n + 1 to 2n); the ties are in the cells, or left out for the yardstick.
    fn pairs(n: u32, tied: bool) -> Document {
        let mut shapes = Vec::new();
        for id in 1..=n {
            let mut target = boxed(id, f64::from(id % 50), f64::from(id / 50));
            if tied {
                tie_cell(&mut target, &format!("SUM(DEPENDSON(3,Sheet.{}!SheetRef()))", n + id));
            }
            shapes.push(target);
        }
        for id in 1..=n {
            let mut callout = boxed(n + id, f64::from(id % 50) + 0.5, f64::from(id / 50) + 0.5);
            if tied {
                tie_cell(&mut callout, &format!("SUM(DEPENDSON(6,Sheet.{id}!SheetRef()))"));
            }
            shapes.push(callout);
        }
        doc_of(shapes)
    }

    fn pin_x(doc: &Document, id: u32) -> f64 {
        Reader::new(doc).number(&SheetRef::Shape(doc.find_shape(Owner::Page(1), id).unwrap()), "PinX")
    }

    #[test]
    fn moving_2000_shapes_that_each_have_a_callout_looks_up_no_shape_for_the_callouts_and_walks_the_page_once() {
        let n = 2_000_u32;
        let ids: Vec<u32> = (1..=n).collect();
        let cost = |tied: bool| {
            let mut doc = pairs(n, tied);
            diacraft_doc::work::reset();
            crate::draw::NODES_WALKED.with(|c| c.set(0));
            assert_eq!(
                crate::draw::move_shapes(&mut doc, 1, &ids, 1.0, 0.0).unwrap(),
                ids.len(),
                "the callouts are not counted among the shapes moved"
            );
            (diacraft_doc::work::finds_visited(), crate::draw::NODES_WALKED.with(std::cell::Cell::get), doc)
        };
        let (plain_finds, plain_walked, _) = cost(false);
        let (finds, walked, doc) = cost(true);
        assert_eq!(
            finds, plain_finds,
            "the callouts are found with the one walk of the page and not looked up from its top each: the yardstick is the move of the same shapes with no callout"
        );
        assert!(walked <= plain_walked + 2 * (2 * n as usize), "one walk of the page for all the callouts: {walked} against {plain_walked}");
        assert!((1..=n).all(|id| (pin_x(&doc, n + id) - (f64::from(id % 50) + 1.5)).abs() < 1e-9), "every callout went the distance of its target");
    }

    #[test]
    fn a_shape_a_file_ties_4000_callouts_to_is_refused_in_words_when_it_is_moved_and_costs_no_search_each() {
        let n = 4_000_u32;
        let mut shapes = vec![boxed(1, 0.0, 0.0)];
        let targets: Vec<String> = (2..=n + 1).map(|id| format!("Sheet.{id}!SheetRef()")).collect();
        tie_cell(&mut shapes[0], &format!("SUM(DEPENDSON(3,{}))", targets.join(",")));
        for id in 2..=n + 1 {
            let mut callout = boxed(id, 1.0, 1.0);
            tie_cell(&mut callout, "SUM(DEPENDSON(6,Sheet.1!SheetRef()))");
            shapes.push(callout);
        }
        let mut doc = doc_of(shapes);
        let before = doc.clone();
        diacraft_doc::work::reset();
        match crate::draw::move_shapes(&mut doc, 1, &[1], 1.0, 0.0) {
            Err(problem) => {
                assert!(problem.message.contains("callouts"), "{}", problem.message);
                assert_eq!(doc, before, "a refusal changes nothing");
            }
            Ok(_) => assert!(
                diacraft_doc::work::finds_visited() <= 2 * u64::from(n),
                "a search of the page for each of {n} callouts: {}",
                diacraft_doc::work::finds_visited()
            ),
        }
    }

    #[test]
    fn copying_a_callout_whose_formula_names_8000_shapes_that_are_not_copied_is_one_pass_over_them() {
        let n = 8_000_u32;
        let targets: Vec<String> = (2..=n + 1).map(|id| format!("Sheet.{id}!SheetRef()")).collect();
        let mut shape = boxed(1, 0.0, 0.0);
        tie_cell(&mut shape, &format!("SUM(DEPENDSON(3,{}),DEPENDSON(6,{}))", targets.join(","), targets.join(",")));
        let copied: HashMap<u32, u32> = [(1, 1)].into_iter().collect();
        assert_eq!(Ties::of_shape(&shape).expect("the formula parses").targets(CALLOUTS).len(), n as usize);
        MARKS_SCANNED.with(|c| c.set(0));
        let cut = cut_uncopied(1, &shape.sheet, &copied).unwrap();
        let scanned = MARKS_SCANNED.with(std::cell::Cell::get);
        assert!(scanned <= 8 * n as usize, "{scanned} marks looked at for {} in the formula", 2 * n);
        let ties = Ties::of_shape(&Shape { sheet: cut, ..Shape::new(1) }).unwrap();
        assert!(ties.targets(CALLOUTS).is_empty() && ties.targets(TARGET).is_empty(), "no shape that is not copied is named by the copy");
    }

    #[test]
    fn a_callout_for_each_of_3000_shapes_finds_the_shapes_with_one_walk_of_the_page_and_searches_for_none() {
        let n = 3_000_u32;
        let mut doc = doc_of((1..=n).map(|id| boxed(id, f64::from(id % 50), f64::from(id / 50))).collect());
        let ids: Vec<u32> = (1..=n).collect();
        diacraft_doc::work::reset();
        crate::draw::NODES_WALKED.with(|c| c.set(0));
        let added = add_callouts(&mut doc, 1, &crate::stencil::CALLOUT_STYLES[2], &ids, None, "Text", 0).unwrap();
        assert_eq!(added.len(), n as usize);
        assert_eq!(diacraft_doc::work::finds_visited(), 0, "no shape is looked for from the top of the page");
        assert!(crate::draw::NODES_WALKED.with(std::cell::Cell::get) <= 2 * n as usize, "one walk of the page for the targets");
        let target = doc.shape(&doc.find_shape(Owner::Page(1), 7).unwrap()).unwrap();
        assert_eq!(Ties::of_shape(target).unwrap().targets(CALLOUTS), [n + 7]);
    }

    #[test]
    fn a_formula_that_names_more_than_20000_shapes_is_read_that_far_and_a_copy_of_it_is_refused_in_words() {
        let list = |from: u32, to: u32| (from..to).map(|id| format!("Sheet.{id}!SheetRef()")).collect::<Vec<_>>().join(",");
        let mut shape = boxed(1, 0.0, 0.0);
        tie_cell(&mut shape, &format!("SUM(DEPENDSON(1,{}),DEPENDSON(2,{}),DEPENDSON(3,Sheet.9!SheetRef()))", list(2, 11_002), list(11_002, 22_002)));
        let ties = Ties::of_shape(&shape).expect("the formula parses");
        assert!(ties.truncated);
        assert_eq!(ties.targets(1).len() + ties.targets(2).len(), MAX_TIE_MARKS, "read as far as the cap");
        // Copied with the callout it names not copied, the formula would have to be rewritten and the rest dropped: refused, with nothing changed.
        let copied: HashMap<u32, u32> = [(1, 1)].into_iter().collect();
        let refusal = cut_uncopied(1, &shape.sheet, &copied).unwrap_err();
        assert!(refusal.message.contains("names more than 20000 shapes"), "{}", refusal.message);
        // So is a copy that would change none of it: what was not read could be a callout. A move of the shape, and a delete of it, are refused the same way.
        let all: HashMap<u32, u32> = (1..=22_002).map(|id| (id, id)).collect();
        assert!(cut_uncopied(1, &shape.sheet, &all).is_err());
        let mut doc = doc_of(vec![shape]);
        let before = doc.clone();
        assert!(crate::draw::move_shapes(&mut doc, 1, &[1], 1.0, 0.0).is_err());
        assert!(crate::draw::delete_shapes(&mut doc, 1, &[1]).is_err());
        assert_eq!(doc, before);
    }

    #[test]
    fn deleting_4000_callouts_a_file_ties_to_one_shape_that_stays_is_one_pass_over_its_ties() {
        let n = 4_000_u32;
        let mut shapes = vec![boxed(1, 0.0, 0.0)];
        let targets: Vec<String> = (2..=n + 1).map(|id| format!("Sheet.{id}!SheetRef()")).collect();
        tie_cell(&mut shapes[0], &format!("SUM(DEPENDSON(3,{}))", targets.join(",")));
        for id in 2..=n + 1 {
            let mut callout = boxed(id, 1.0, 1.0);
            tie_cell(&mut callout, "SUM(DEPENDSON(6,Sheet.1!SheetRef()))");
            shapes.push(callout);
        }
        let mut doc = doc_of(shapes);
        let ids: Vec<u32> = (2..=n + 1).collect();
        MARKS_SCANNED.with(|c| c.set(0));
        crate::draw::delete_shapes(&mut doc, 1, &ids).unwrap();
        let scanned = MARKS_SCANNED.with(std::cell::Cell::get);
        assert!(scanned <= 8 * n as usize, "{scanned} marks looked at to take {n} callouts out of one shape's ties");
        assert!(doc.find_shape(Owner::Page(1), 1).is_some());
        let target = doc.shape(&doc.find_shape(Owner::Page(1), 1).unwrap()).unwrap();
        assert!(Ties::of_shape(target).unwrap().targets(CALLOUTS).is_empty(), "the shape that stays names none of them");
    }

    // ---- one walk for the callouts and the members ---------------------------------------------------------------

    /// A container (`ID` `id`) that holds `members`, each of which names it back under 4.
    fn container_of(id: u32, members: &[u32], x: f64) -> Shape {
        let mut shape = boxed(id, x, 0.0);
        let list: Vec<String> = members.iter().map(|m| format!("Sheet.{m}!SheetRef()")).collect();
        tie_cell(&mut shape, &format!("SUM(DEPENDSON(1,{}))", list.join(",")));
        shape
    }

    fn member_of(id: u32, container: u32, x: f64, extra: &str) -> Shape {
        let mut shape = boxed(id, x, 0.0);
        tie_cell(&mut shape, &format!("SUM(DEPENDSON(4,Sheet.{container}!SheetRef()){extra})"));
        shape
    }

    #[test]
    fn a_container_that_moves_takes_its_members_and_what_is_tied_to_them_each_once_with_each_shapes_ties_read_once() {
        // 1 holds 2, 3 and the container 6, which holds 7; 4 is a callout of 2, 5 a callout of the callout 4, 8 a callout of 7.
        let mut shapes = vec![
            container_of(1, &[2, 3, 6], 0.0),
            member_of(2, 1, 1.0, ",DEPENDSON(3,Sheet.4!SheetRef())"),
            member_of(3, 1, 2.0, ""),
            boxed(9, 50.0, 0.0),
        ];
        let mut callout4 = boxed(4, 3.0, 0.0);
        tie_cell(&mut callout4, "SUM(DEPENDSON(6,Sheet.2!SheetRef()),DEPENDSON(3,Sheet.5!SheetRef()))");
        let mut callout5 = boxed(5, 4.0, 0.0);
        tie_cell(&mut callout5, "SUM(DEPENDSON(6,Sheet.4!SheetRef()))");
        let mut inner = container_of(6, &[7], 5.0);
        tie_cell(&mut inner, "SUM(DEPENDSON(1,Sheet.7!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))");
        let seven = member_of(7, 6, 6.0, ",DEPENDSON(3,Sheet.8!SheetRef())");
        let mut callout8 = boxed(8, 7.0, 0.0);
        tie_cell(&mut callout8, "SUM(DEPENDSON(6,Sheet.7!SheetRef()))");
        shapes.extend([callout4, callout5, inner, seven, callout8]);
        let mut doc = doc_of(shapes);
        let at = doc.find_shape(Owner::Page(1), 1).unwrap();
        TIES_READ.with(|c| c.set(0));
        let walked = followers(&doc, 1, &[(1, at, (1.0, 0.0))]).unwrap();
        let reads = TIES_READ.with(std::cell::Cell::get);
        let mut ids: Vec<u32> = walked.iter().map(|(id, _, _)| *id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [2, 3, 4, 5, 6, 7, 8], "the members, the callouts of the members and the callouts of those, the container inside too");
        assert!(walked.iter().all(|(_, _, d)| *d == (1.0, 0.0)));
        assert!(reads <= walked.len() + 1, "the ties of each shape are read once: {reads} reads for {} shapes", walked.len() + 1);
        // Moved: every one of them by the distance once, and the shape outside none.
        crate::draw::move_shapes(&mut doc, 1, &[1], 1.0, 0.0).unwrap();
        for (id, was) in [(1, 0.0), (2, 1.0), (3, 2.0), (4, 3.0), (5, 4.0), (6, 5.0), (7, 6.0), (8, 7.0), (9, 50.0)] {
            let now = pin_x(&doc, id);
            let want = if id == 9 { was } else { was + 1.0 };
            assert!((now - want).abs() < 1e-9, "shape {id} went to {now}, not {want}");
        }
    }

    #[test]
    fn a_member_and_its_container_moved_together_go_once() {
        let mut doc = doc_of(vec![container_of(1, &[2], 0.0), member_of(2, 1, 1.0, "")]);
        crate::draw::move_shapes(&mut doc, 1, &[1, 2], 1.0, 0.0).unwrap();
        assert!((pin_x(&doc, 1) - 1.0).abs() < 1e-9 && (pin_x(&doc, 2) - 2.0).abs() < 1e-9, "each by the distance, once");
    }

    #[test]
    fn containers_a_thousand_deep_move_with_two_walks_of_the_page_and_a_container_that_is_not_a_plain_list_takes_none() {
        let n = 1_000_u32;
        let mut shapes = Vec::new();
        for id in 1..=n {
            let mut shape = boxed(id, f64::from(id % 50), f64::from(id / 50));
            let down = if id < n { format!("DEPENDSON(1,Sheet.{}!SheetRef())", id + 1) } else { "DEPENDSON(1)".to_string() };
            let up = if id > 1 { format!(",DEPENDSON(4,Sheet.{}!SheetRef())", id - 1) } else { String::new() };
            tie_cell(&mut shape, &format!("SUM({down}{up})"));
            shapes.push(shape);
        }
        let mut doc = doc_of(shapes);
        diacraft_doc::work::reset();
        crate::draw::NODES_WALKED.with(|c| c.set(0));
        assert_eq!(crate::draw::move_shapes(&mut doc, 1, &[1], 1.0, 0.0).unwrap(), 1);
        let walked = crate::draw::NODES_WALKED.with(std::cell::Cell::get);
        assert!(walked <= 4 * n as usize, "one walk for the first round and one index for the rest, not one for each of {n} levels: {walked}");
        assert!(diacraft_doc::work::finds_visited() <= 2, "the shape named is looked for, the rest are not: {}", diacraft_doc::work::finds_visited());
        assert!((1..=n).all(|id| (pin_x(&doc, id) - (f64::from(id % 50) + 1.0)).abs() < 1e-9), "every level went the distance once");
        // A formula with anything else in it is not read as a container's list: its members stay.
        let mut odd = doc_of(vec![boxed(1, 0.0, 0.0), boxed(2, 1.0, 0.0)]);
        tie_cell(super::page_mut(&mut odd, 1).unwrap().shapes.get_mut(0).map(Arc::make_mut).unwrap(), "IF(1,DEPENDSON(1,Sheet.2!SheetRef()),0)");
        crate::draw::move_shapes(&mut odd, 1, &[1], 1.0, 0.0).unwrap();
        assert!((pin_x(&odd, 2) - 1.0).abs() < 1e-9, "no member of a formula that is not a list of ties");
    }

    #[test]
    fn many_containers_that_hold_the_same_shapes_name_each_once_in_the_walk() {
        let (containers, held) = (3_000_u32, 50_u32);
        let members: Vec<u32> = (containers + 1..=containers + held).collect();
        let mut shapes: Vec<Shape> = (1..=containers).map(|id| container_of(id, &members, f64::from(id % 100))).collect();
        shapes.extend(members.iter().map(|m| member_of(*m, 1, 200.0, "")));
        let doc = doc_of(shapes);
        let ids: Vec<u32> = (1..=containers).collect();
        let moved: Vec<(u32, ShapeRef, P)> = ids.iter().map(|id| (*id, doc.find_shape(Owner::Page(1), *id).unwrap(), (1.0, 0.0))).collect();
        let walked = followers(&doc, 1, &moved).unwrap();
        assert_eq!(walked.len(), held as usize, "each shape the containers hold goes once, whatever the number of containers that hold it");
        let unique: HashSet<u32> = walked.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(unique.len(), walked.len());
    }

    #[test]
    fn more_than_the_most_that_follow_an_edit_are_refused_in_words_with_nothing_moved() {
        let n = MAX_FOLLOWING as u32 + 1;
        let members: Vec<u32> = (2..=n + 1).collect();
        // A formula names at most MAX_TIE_MARKS shapes, so the members are held by two containers.
        let (a, b) = members.split_at(members.len() / 2);
        let mut shapes = vec![container_of(1, a, 0.0)];
        shapes.push(container_of(n + 2, b, 0.0));
        shapes.extend(members.iter().map(|m| boxed(*m, 1.0, 1.0)));
        let mut doc = doc_of(shapes);
        let before = doc.clone();
        let refusal = crate::draw::move_shapes(&mut doc, 1, &[1, n + 2], 1.0, 0.0).unwrap_err();
        assert!(refusal.message.contains("would follow"), "{}", refusal.message);
        assert_eq!(doc, before);
    }

    #[test]
    fn a_number_that_is_not_a_whole_code_is_no_call() {
        assert!(ties_of("SUM(DEPENDSON(3.5,Sheet.7!SheetRef()),DEPENDSON(-1,Sheet.2!SheetRef()))").calls.is_empty());
    }

    #[test]
    fn a_loop_of_containers_put_in_by_hand_moves_each_once_with_each_shapes_ties_read_once() {
        // No command makes a loop, and the walk stays bounded with one all the same: the one moved is not taken again by the last, which holds it.
        let n = 1_000_u32;
        let shapes: Vec<Shape> = (1..=n)
            .map(|id| {
                let mut shape = boxed(id, f64::from(id % 50), f64::from(id / 50));
                let (next, before) = (id % n + 1, if id == 1 { n } else { id - 1 });
                tie_cell(&mut shape, &format!("SUM(DEPENDSON(1,Sheet.{next}!SheetRef()),DEPENDSON(4,Sheet.{before}!SheetRef()))"));
                shape
            })
            .collect();
        let mut doc = doc_of(shapes);
        let at = doc.find_shape(Owner::Page(1), 1).unwrap();
        TIES_READ.with(|c| c.set(0));
        crate::draw::NODES_WALKED.with(|c| c.set(0));
        let walked = followers(&doc, 1, &[(1, at, (1.0, 0.0))]).unwrap();
        let reads = TIES_READ.with(std::cell::Cell::get);
        let mut ids: Vec<u32> = walked.iter().map(|(id, _, _)| *id).collect();
        ids.sort_unstable();
        assert_eq!(ids, (2..=n).collect::<Vec<u32>>(), "every other container of the loop, each once");
        assert!(reads <= walked.len() + 1, "the ties of each shape are read once: {reads} reads for {} shapes", walked.len() + 1);
        let nodes = crate::draw::NODES_WALKED.with(std::cell::Cell::get);
        assert!(nodes <= 4 * n as usize, "one walk for the first round and one index for the rest: {nodes}");
        crate::draw::move_shapes(&mut doc, 1, &[1], 1.0, 0.0).unwrap();
        assert!((1..=n).all(|id| (pin_x(&doc, id) - (f64::from(id % 50) + 1.0)).abs() < 1e-9), "each went the distance once");
    }
}
