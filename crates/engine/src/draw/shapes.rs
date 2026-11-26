//! Shapes: drop an instance of a master, move, resize, delete. (The text of a shape is `text_edit`.)

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use diacraft_calc::{Budget, CellAddress, EvalCtx, Value as Calc};
use diacraft_doc::{
    Cell, CellAddr, CellEdit, Document, FormulaSlot, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, Shape, ShapeList, ShapeRef, ShapeType, SheetRef,
    Value,
};

use super::jumps;
use super::read::{Pose, Reader};
use super::route::P;
use super::scan;
use super::settle::settle;
use super::{Problem, checked, page_mut, require_page, require_shape, shape_mut, write_cells, write_length};
use crate::stencil::{self, MasterSpec};

/// What a drop made.
#[derive(Debug, Clone, PartialEq)]
pub struct DropInfo {
    /// The new shape's `ID` on the page.
    pub id: u32,
    /// The master's `ID` in the document.
    pub master: u32,
    pub name: String,
    /// The size the instance has, which is the master's.
    pub width: f64,
    pub height: f64,
}

/// The highest `ID` on a page, nested shapes counted, and how many shapes it holds. Iterative, and no deeper than the model allows.
pub(crate) fn page_stats(doc: &Document, page: u32) -> Option<(u32, usize)> {
    let top = &doc.page(page)?.shapes;
    let mut stack: Vec<(&ShapeList, usize)> = vec![(top, 1)];
    let (mut highest, mut count) = (0_u32, 0_usize);
    while let Some((list, depth)) = stack.pop() {
        for shape in list.iter() {
            highest = highest.max(shape.id);
            count = count.saturating_add(1);
            if depth < MAX_SHAPE_DEPTH && !shape.children.is_empty() {
                stack.push((&shape.children, depth + 1));
            }
        }
    }
    Some((highest, count))
}

// How many shapes the walks of subtrees have visited, for the tests that hold a command to walking each shape of a page once.
#[cfg(test)]
thread_local! {
    pub(crate) static SUBTREE_NODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The `ID`s of a shape and everything inside it.
pub(super) fn subtree_ids(shape: &Shape) -> Vec<u32> {
    let mut out = vec![shape.id];
    let mut stack: Vec<(&ShapeList, usize)> = vec![(&shape.children, 1)];
    while let Some((list, depth)) = stack.pop() {
        for member in list.iter() {
            #[cfg(test)]
            SUBTREE_NODES.with(|c| c.set(c.get() + 1));
            out.push(member.id);
            if depth < MAX_SHAPE_DEPTH && !member.children.is_empty() {
                stack.push((&member.children, depth + 1));
            }
        }
    }
    out
}

/// Drops an instance of `spec` centred at (`x`, `y`) on a page: the master is copied into the document if it is not there, and the
/// instance holds only its pin (what an instance can inherit it does not copy). Nothing needs recalculating: the
/// instance has the master's size, so every inherited result is the master's own. The new shape's `ID` is the highest on the page plus 1; a
/// caller that keeps the `ID`s it has given (the session does) uses [`drop_shape_above`].
pub fn drop_shape(doc: &mut Document, page: u32, spec: &MasterSpec, x: f64, y: f64) -> Result<DropInfo, Problem> {
    drop_shape_above(doc, page, spec, x, y, 0)
}

/// [`drop_shape`] with an `ID` above `above` as well as above every `ID` on the page: the highest `ID` the caller has given on the page
/// or that its baseline file holds (the session's **high-water mark**, `Session::id_floor`). An `ID` that was a shape's is then never a
/// new shape's, though the shape is gone: a formula that named it does not name the new one, and the writer does not meet a new shape where
/// the file had a picture.
pub fn drop_shape_above(doc: &mut Document, page: u32, spec: &MasterSpec, x: f64, y: f64, above: u32) -> Result<DropInfo, Problem> {
    let (x, y) = (checked("x", x)?, checked("y", y)?);
    require_page(doc, page)?;
    if spec.is_connector() {
        return drop_connector(doc, page, spec, (x, y), above);
    }
    // A group master is dropped with a member for each member of the master, as a stencil file's group master is (`stencil/instance.rs`).
    if spec.is_group() {
        return stencil::drop_group_master(doc, page, spec, (x, y), above);
    }
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let master = stencil::ensure_master(doc, spec)?;
    let mut shape = Shape::new(id);
    let name: Arc<str> = Arc::from(format!("{}.{id}", spec.name));
    shape.name = Some(Arc::clone(&name));
    shape.name_u = Some(Arc::clone(&name));
    shape.kind = ShapeType::Shape;
    shape.master = Some(master);
    let mut sheet = diacraft_doc::Sheet::default();
    for (cell, value) in [("PinX", x), ("PinY", y)] {
        sheet.cells.insert(diacraft_doc::CellName::new(cell), Cell::constant(Value::from_text(&stencil::number_text(value))).with_unit("IN"));
    }
    // A position of the Organization Chart Shapes keeps the text that shows its name and title (the shown form of a field lives in the text that holds it) and its marker.
    if let Some((text, marker)) = stencil::orgchart::dressing(spec) {
        sheet.sections.push(Arc::new(marker));
        shape.text = Some(Arc::new(text));
    } else if let Some(text) = stencil::dropped_text(spec) {
        // A master whose text holds fields (the `Dress`): the shown form of a field lives in the text that holds it.
        shape.text = Some(Arc::new(text));
    }
    shape.sheet = Arc::new(sheet);
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.shapes.push(Arc::new(shape));
    Ok(DropInfo { id, master, name: name.to_string(), width: spec.width, height: spec.height })
}

/// Drops a one-dimensional master: its ends are the master's width apart, centred on (`x`, `y`) and level. **The instance inherits the master's look** (its line ends, pattern, weight and route):
/// the arrowhead the connector tool writes on a connector of its own is taken out, so that the master's cells are what draws. A connector master is routed as any
/// connector is. **A `Fixed1D` master is not routed**: it has no route marker and none of the route's own geometry, so its geometry is the master's, stretched along the line as its ends
/// move, and the master's `ShapeRouteStyle` 16 keeps a reroute from laying a path over it.
fn drop_connector(doc: &mut Document, page: u32, spec: &MasterSpec, (x, y): (f64, f64), above: u32) -> Result<DropInfo, Problem> {
    let half = spec.width / 2.0;
    let begin = super::End::Point { x: checked("begin x", x - half)?, y };
    let end = super::End::Point { x: checked("end x", x + half)?, y };
    let master = stencil::ensure_master(doc, spec)?;
    let before = doc.clone();
    let info = if spec.is_fixed_1d() {
        super::attach_connector_above(doc, page, &begin, &end, above)?
    } else {
        super::add_connector_above(doc, page, &begin, &end, above)?
    };
    let at = require_shape(doc, page, info.id)?;
    let shape = shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("the connector went away"))?;
    let name: Arc<str> = Arc::from(format!("{}.{id}", spec.name, id = info.id));
    shape.master = Some(master);
    shape.name = Some(Arc::clone(&name));
    shape.name_u = Some(Arc::clone(&name));
    let sheet = Arc::make_mut(&mut shape.sheet);
    sheet.cells.remove(&diacraft_doc::CellName::new("EndArrow"));
    if spec.is_fixed_1d() {
        // No route marker, and no route section: the geometry is the master's.
        sheet.sections.retain(|section| !matches!(section.name.as_str(), "User" | "Geometry"));
        settle(doc, &before, page, &[info.id], &[])?;
    }
    Ok(DropInfo { id: info.id, master, name: name.to_string(), width: spec.width, height: spec.height })
}

/// Moves shapes by (`dx`, `dy`). A shape moves by its pin; a one-dimensional shape (a connector) moves its ends that are not glued, and
/// the glued ones stay where their targets hold them (**MOVE-1D**, ours: moving a glued end would break the glue). Connectors glued
/// to a moved shape follow by their formulas and are rerouted.
///
/// (`dx`, `dy`) is a distance on the page. A member of a group has its pin in the group's frame, which is the page's only for a group that is
/// not turned or flipped, so the distance is carried into the frame of the shape's parent first (of the M3 review).
///
/// → how many of the shapes named moved: a connector whose two ends are glued has no end of its own to move (it follows its shapes), and when
/// nothing moves the document is not touched, so that the caller can leave it as it was.
pub fn move_shapes(doc: &mut Document, page: u32, ids: &[u32], dx: f64, dy: f64) -> Result<usize, Problem> {
    let (dx, dy) = (checked("dx", dx)?, checked("dy", dy)?);
    let ids: BTreeSet<u32> = ids.iter().copied().collect();
    let moves: Vec<(u32, P)> = ids.into_iter().map(|id| (id, (dx, dy))).collect();
    move_each(doc, page, &moves)
}

/// What a move a person asks for did ([`move_shapes_leaving`]): how many of the shapes named moved, and the shapes that follow them that were left where they were, each with the
/// first cell of its move that a `GUARD` protects.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ShapesMoved {
    pub moved: usize,
    pub left: Vec<(u32, &'static str)>,
}

/// [`move_shapes`] for a move a person asks for (`shape.move`), **which leaves a follower whose move a `GUARD` refuses**: a shape that
/// follows the shapes moved (a member of a container moved, or a callout tied to a shape moved) is moved by writing its pin, or the ends of a one-dimensional shape that are not
/// glued (MOVE-1D), and one of whose cells a `GUARD` protects is left where it is, with what follows only it, and named in the answer, where [`move_shapes`] refuses the whole move;
/// the shapes named and every other follower move. A shape named whose own cell is guarded still refuses the move, and so does a follower on a locked layer, in the words that
/// name its layer (unchanged).
pub fn move_shapes_leaving(doc: &mut Document, page: u32, ids: &[u32], dx: f64, dy: f64) -> Result<ShapesMoved, Problem> {
    let (dx, dy) = (checked("dx", dx)?, checked("dy", dy)?);
    let ids: BTreeSet<u32> = ids.iter().copied().collect();
    // Found in one walk of the page, where a search for each walked the page from its top for each; the lowest `ID` missing is refused, as it was.
    let named = scan::Named::of(doc, page, ids.iter().copied())?;
    let mut paths: Vec<(u32, ShapeRef, P)> = Vec::with_capacity(ids.len());
    for id in ids {
        paths.push((id, named.require(id)?, (dx, dy)));
    }
    let mut follow = super::callouts::followers(doc, page, &paths)?;
    let left = guarded_followers(doc, page, &follow);
    if !left.is_empty() {
        // The walk again, passing over the shapes left: what follows only them stays with them. It reaches no shape the first did not, so none of these is guarded (a write
        // that a guard still refused would refuse the move in words, never leave it half made).
        let passed: HashSet<u32> = left.iter().map(|(id, _)| *id).collect();
        follow = super::callouts::followers_leaving(doc, page, &paths, &passed)?;
    }
    let named = paths.len();
    paths.extend(follow);
    let moved = move_planned(doc, page, &paths, named)?;
    Ok(ShapesMoved { moved, left })
}

/// The followers `follow` (each with where it is) whose move a `GUARD` refuses, each with the first cell of its move that one protects: the cells [`write_moves`] would write.
fn guarded_followers(doc: &Document, page: u32, follow: &[(u32, ShapeRef, P)]) -> Vec<(u32, &'static str)> {
    let reader = Reader::new(doc);
    let mut out: Vec<(u32, &'static str)> = Vec::new();
    let mut looked: HashSet<u32> = HashSet::new();
    for (id, at, _) in follow {
        if !looked.insert(*id) {
            continue;
        }
        let sheet = SheetRef::Shape(at.clone());
        let guarded = |name: &str| {
            reader.resolver().resolve(&sheet, &CellAddr::direct(name)).is_some_and(|found| found.formula.cell.is_some_and(Cell::is_guarded))
        };
        if let Some(cell) = moved_cells(doc, page, &reader, *id, &sheet).into_iter().flat_map(|(x, y)| [x, y]).find(|name| guarded(name)) {
            out.push((*id, cell));
        }
    }
    out
}

/// The words of a move's answer for the followers it left: up to five named, each with its cell, and how many more. What was left and why come first, since the
/// window's status bar shows the start of a long note.
pub(crate) fn left_words(left: &[(u32, &'static str)]) -> Option<String> {
    const NAMED: usize = 5;
    match left {
        [] => None,
        [(id, cell)] => Some(format!(
            "shape {id} was left where it was: a GUARD protects its cell `{cell}`, which the move would write (it follows the shapes moved, as a member of a container or a callout)"
        )),
        _ => {
            let named: Vec<String> = left.iter().take(NAMED).map(|(id, cell)| format!("shape {id} (`{cell}`)")).collect();
            let more = left.len().saturating_sub(NAMED);
            let tail = if more > 0 { format!(", and {more} more") } else { String::new() };
            Some(format!(
                "{} shapes were left where they were: a GUARD protects a cell the move of each would write, {}{tail} (they follow the shapes moved, as members of a container or callouts)",
                left.len(),
                named.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod left_words_tests {
    use super::left_words;

    /// The note names what was left and why first, five shapes at most, and counts the rest.
    #[test]
    fn the_note_names_the_shapes_left_and_their_cells_and_counts_past_five() {
        assert_eq!(left_words(&[]), None);
        let one = left_words(&[(3, "BeginX")]).unwrap();
        assert!(one.starts_with("shape 3 was left where it was: a GUARD protects its cell `BeginX`"), "{one}");
        let many: Vec<(u32, &'static str)> = (1..=7).map(|id| (id, "PinX")).collect();
        let words = left_words(&many).unwrap();
        assert!(words.starts_with("7 shapes were left where they were: a GUARD protects"), "{words}");
        assert!(words.contains("shape 5 (`PinX`), and 2 more") && !words.contains("shape 6"), "{words}");
    }
}

/// [`move_shapes`] with a distance of its own for each shape (`shape.align`): each `(id, (dx, dy))` moves that shape by that distance
/// on the page, by the same rules (**MOVE-1D**), and the document is recalculated and the connectors routed once. A shape
/// named twice moves twice.
pub(super) fn move_each(doc: &mut Document, page: u32, moves: &[(u32, P)]) -> Result<usize, Problem> {
    // Found in one walk of the page, where a search for each walked the page from its top for each; each move is checked in its order, as it was.
    let named = scan::Named::of(doc, page, moves.iter().map(|(id, _)| *id))?;
    let mut paths: Vec<(u32, ShapeRef, P)> = Vec::with_capacity(moves.len());
    for (id, d) in moves {
        paths.push((*id, named.require(*id)?, (checked("dx", d.0)?, checked("dy", d.1)?)));
    }
    // The callouts tied to the shapes moved go with them, by the same distance (`callouts.rs`); they are not counted among the shapes moved. Their places come with them, found by the
    // one walk of the page that finding them costs, so none is searched for again.
    let follow = super::callouts::followers(doc, page, &paths)?;
    let named = paths.len();
    paths.extend(follow);
    move_planned(doc, page, &paths, named)
}

/// [`move_each`] for shapes found already, each with its place and its distance, and the first `named` of them counted in the answer.
pub(super) fn move_planned(doc: &mut Document, page: u32, paths: &[(u32, ShapeRef, P)], named: usize) -> Result<usize, Problem> {
    let before = doc.clone();
    let (moved, written) = write_moves_counting(doc, page, paths, named)?;
    if !written {
        return Ok(0);
    }
    let touched: Vec<u32> = paths.iter().map(|(id, _, _)| *id).collect::<BTreeSet<u32>>().into_iter().collect();
    settle(doc, &before, page, &touched, &[])?;
    Ok(moved)
}

/// Writes the cells that move the shapes `paths` (each with its place and its distance on the page, by the rules of **MOVE-1D**) and **nothing else**: the caller
/// settles the page (`draw::settle`) once for all it changed. → how many shapes were written.
pub(super) fn write_moves(doc: &mut Document, page: u32, paths: &[(u32, ShapeRef, P)]) -> Result<usize, Problem> {
    Ok(write_moves_counting(doc, page, paths, paths.len())?.0)
}

/// [`write_moves`], counting the first `named` shapes, and saying whether any cell was written.
fn write_moves_counting(doc: &mut Document, page: u32, paths: &[(u32, ShapeRef, P)], named: usize) -> Result<(usize, bool), Problem> {
    let mut plan: Vec<(SheetRef, &'static str, f64)> = Vec::new();
    let mut moved = 0_usize;
    {
        for (_, _, d) in paths {
            checked("dx", d.0)?;
            checked("dy", d.1)?;
        }
        let reader = Reader::new(doc);
        for (k, (id, at, d)) in paths.iter().enumerate() {
            let sheet = SheetRef::Shape(at.clone());
            let (dx, dy) = reader.vector_from_page(at, *d);
            let planned = plan.len();
            for (x_cell, y_cell) in moved_cells(doc, page, &reader, *id, &sheet) {
                plan.push((sheet.clone(), x_cell, reader.number(&sheet, x_cell) + dx));
                plan.push((sheet.clone(), y_cell, reader.number(&sheet, y_cell) + dy));
            }
            if k < named {
                moved = moved.saturating_add(usize::from(plan.len() > planned));
            }
        }
    }
    if plan.is_empty() {
        return Ok((0, false));
    }
    for (sheet, name, value) in plan {
        write_length(doc, &sheet, name, checked(name, value)?)?;
    }
    Ok((moved, true))
}

/// The pairs of cells a move of the shape `id` (at `sheet`) writes, each moved by the distance on its axis: its pin, or for a one-dimensional shape the ends that are not glued
/// (**MOVE-1D**).
fn moved_cells(doc: &Document, page: u32, reader: &Reader<'_>, id: u32, sheet: &SheetRef) -> Vec<(&'static str, &'static str)> {
    if !reader.is_one_d(sheet) {
        return vec![("PinX", "PinY")];
    }
    let glued =
        |from_cell: &str| doc.page(page).is_some_and(|p| p.connects.iter().any(|c| c.from_sheet == id && c.from_cell.as_deref() == Some(from_cell)));
    [("BeginX", "BeginY"), ("EndX", "EndY")].into_iter().filter(|(x_cell, _)| !glued(x_cell)).collect()
}

/// Sets a shape's frame: its box is `width` by `height` with the corner at (0, 0) of its own frame at (`x`, `y`) on the page (ours;  it was, which refused a turned or flipped shape).
/// For a shape that is not turned or flipped that corner is the lower-left one, as it
/// was. For one that is, `width` and `height` are along the shape's own axes and the box keeps its turn and its flips: the pin follows so that the
/// corner lands there (the local pin is worked out again for the new size, so a pin at the middle stays at the middle, and the pin is carried
/// from the corner by the shape's turn and flips). A connector is refused, and a group is sized as a shape and its members follow (`draw/scale.rs`). (`x`, `y`) is a point
/// of the page; the pin of a member of a group is in the group's frame, so the point is carried into the frame of the shape's parent first, and a
/// member of a group that is turned or flipped is refused, since its box is not square to the page (of the M3 review).
pub fn resize_shape(doc: &mut Document, page: u32, id: u32, x: f64, y: f64, width: f64, height: f64) -> Result<(), Problem> {
    let before = doc.clone();
    let Resized { at, group, old_size, kept } = resize_cells(doc, page, id, x, y, width, height)?;
    // A group is one shape: its members follow its frame, by the formulas that tie them to it and, where they hold numbers, by the stretch of the frame. A size the shape keeps
    // is not stretched.
    let mut member_lines: Vec<u32> = Vec::new();
    if group {
        let mut left = super::scale::MAX_SCALED_MEMBERS;
        let new_size = (if kept.0 { old_size.0 } else { width }, if kept.1 { old_size.1 } else { height });
        super::scale::scale_members_of(doc, &at, old_size, new_size, &mut left, &mut member_lines)?;
    }
    settle(doc, &before, page, &[id], &member_lines)
}

/// What [`resize_cells`] wrote: where the shape is, whether it is a group (its members are then not scaled: that is for [`resize_shape`]), its old size, and which sizes it keeps
/// (width, height: [`keeps`]).
pub(super) struct Resized {
    pub(super) at: ShapeRef,
    pub(super) group: bool,
    pub(super) old_size: (f64, f64),
    pub(super) kept: (bool, bool),
}

/// The writes of [`resize_shape`] and no recalculation after them: the cells of the frame (`Width`, `Height` and the pin, which is placed with the local pin the new size
/// gives), for a caller that sizes many shapes and settles once (a fit of containers, `draw/container/fit.rs`).
pub(super) fn resize_cells(doc: &mut Document, page: u32, id: u32, x: f64, y: f64, width: f64, height: f64) -> Result<Resized, Problem> {
    let (x, y, width, height) = (checked("x", x)?, checked("y", y)?, checked("width", width)?, checked("height", height)?);
    if width <= 0.0 || height <= 0.0 {
        return Err(Problem::bad("`width` and `height` must be more than 0"));
    }
    let at = require_shape(doc, page, id)?;
    let sheet = SheetRef::Shape(at.clone());
    let (group, old_size, kept);
    let ((x, y), pose) = {
        let reader = Reader::new(doc);
        if reader.is_one_d(&sheet) {
            return Err(Problem::failed("a connector is resized by moving its ends"));
        }
        if reader.groups_turn(&at) {
            return Err(Problem::failed("a member of a turned or flipped group cannot be resized yet"));
        }
        group = doc.shape(&at).is_some_and(|s| !s.children.is_empty());
        old_size = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
        kept = kept_sizes(&reader, &sheet);
        (reader.point_from_page(&at, (x, y)), reader.pose(&sheet))
    };
    // A group's members follow its size: one on a locked layer is not resized through it.
    if group {
        super::layers::refuse_locked_member(doc, page, &[id], "resized")?;
    }
    if !kept.0 {
        write_length(doc, &sheet, "Width", width)?;
    }
    if !kept.1 {
        write_length(doc, &sheet, "Height", height)?;
    }
    // The local pin for the new size, worked out on its own: the pin is placed with it, and the page is recalculated once.
    let loc_x = evaluate_number(doc, &sheet, "LocPinX").unwrap_or_else(|| Reader::new(doc).number(&sheet, "LocPinX"));
    let loc_y = evaluate_number(doc, &sheet, "LocPinY").unwrap_or_else(|| Reader::new(doc).number(&sheet, "LocPinY"));
    // The pin is the local pin carried from the corner by the turn and the flips (nothing for a shape that has neither).
    let (to_pin_x, to_pin_y) = pose.up_vector((loc_x, loc_y));
    let pin = (x + to_pin_x, y + to_pin_y);
    let (pin_x, pin_y) = if kept.0 || kept.1 { held_pin(&pose, pin, kept) } else { pin };
    write_length(doc, &sheet, "PinX", checked("x", pin_x)?)?;
    write_length(doc, &sheet, "PinY", checked("y", pin_y)?)?;
    Ok(Resized { at, group, old_size, kept })
}

/// Which sizes of a shape a resize of it leaves alone: `(width, height)`, each by [`keeps`].
pub(super) fn kept_sizes(reader: &Reader<'_>, sheet: &SheetRef) -> (bool, bool) {
    (keeps(reader, sheet, "LockWidth", "Width"), keeps(reader, sheet, "LockHeight", "Height"))
}

/// Whether a resize of one shape leaves its `name` size cell (`Width` or `Height`) alone: its lock (`LockWidth`, `LockHeight`) is set **and the cell holds a
/// formula**, which is what a group made of compartments has for its height (the sum of theirs), and which a write would replace or a `GUARD` would refuse. A locked size that is a number is
/// written as it always was: `shape.resize` with one `id` and the window's handles do not read the locks otherwise (those with `ids` do, `draw/scale.rs`).
fn keeps(reader: &Reader<'_>, sheet: &SheetRef, lock: &str, name: &str) -> bool {
    reader.number(sheet, lock) != 0.0
        && reader
            .resolver()
            .resolve(sheet, &CellAddr::direct(name))
            .is_some_and(|found| found.formula.cell.is_some_and(|cell| matches!(cell.formula, FormulaSlot::Local(_) | FormulaSlot::Inherit)))
}

/// The pin a resize puts a shape at when it keeps a size: the pin the box asks for, held where it is along each axis of the shape's own frame that is kept. A shape whose height is its text's keeps
/// the pin it has on that axis (its top edge if the pin is there, its middle if it is in the middle), and the text that wraps differently in a new width moves nothing of it.
fn held_pin(pose: &Pose, want: P, (width, height): (bool, bool)) -> P {
    let (dx, dy) = pose.down_vector((want.0 - pose.pin.0, want.1 - pose.pin.1));
    let (dx, dy) = (if width { 0.0 } else { dx }, if height { 0.0 } else { dy });
    let (ux, uy) = pose.up_vector((dx, dy));
    (pose.pin.0 + ux, pose.pin.1 + uy)
}

/// What a cell directly under `sheet` works out to now, if it is a number.
pub(super) fn evaluate_number(doc: &Document, sheet: &SheetRef, name: &str) -> Option<f64> {
    let mut ctx = EvalCtx::new(crate::measure::eval_options(), Budget::default());
    match diacraft_calc::evaluate(doc, &CellAddress::new(sheet.clone(), CellAddr::direct(name)), &mut ctx) {
        Calc::Number(q) if q.stored.is_finite() => Some(q.stored),
        _ => None,
    }
}

/// Deletes shapes and everything inside them. A connector glued to a deleted shape stays, and the end that was glued becomes a plain
/// point where it was (ours); its `Connect` row goes. The `Connect` rows of a deleted connector go too. Masters stay in the
/// document whether or not an instance is left. → how many shapes were named, an `ID` named twice counted
/// once (of the M3 review).
pub fn delete_shapes(doc: &mut Document, page: u32, ids: &[u32]) -> Result<usize, Problem> {
    let ids: BTreeSet<u32> = ids.iter().copied().collect();
    let mut paths: Vec<Vec<u32>> = Vec::with_capacity(ids.len());
    let mut gone: BTreeSet<u32> = BTreeSet::new();
    // Found in one walk of the page, where a search for each walked the page from its top for each; the lowest `ID` missing is refused, as it was.
    let named = scan::Named::of(doc, page, ids.iter().copied())?;
    for id in &ids {
        let at = named.require(*id)?;
        if let Some(shape) = doc.shape(&at) {
            gone.extend(subtree_ids(shape));
        }
        paths.push(at.path);
    }
    delete_paths_with_callouts(doc, page, paths, &gone)?;
    Ok(ids.len())
}

/// Takes out the shapes at `paths` (their members go with them), whose `ID`s and those of everything inside them are `gone`: the rest of
/// [`delete_shapes`], for a caller that has found the shapes already (`edit.cut`, which found them in one walk of the page).
pub(super) fn delete_paths(doc: &mut Document, page: u32, paths: Vec<Vec<u32>>, gone: &BTreeSet<u32>) -> Result<(), Problem> {
    delete_inner(doc, page, paths, gone, false)
}

/// [`delete_paths`] for a delete the person asks for (`shape.delete`, a cut, the shapes of a layer): **the callouts tied to the shapes that go go with them**, and a callout that goes is taken out of the
/// `Relationships` of the shape it points at, which stays (`callouts.rs`). A shape operation deletes its originals with [`delete_paths`]: their callouts are left, tied to nothing, as
/// says.
pub(super) fn delete_paths_with_callouts(doc: &mut Document, page: u32, paths: Vec<Vec<u32>>, gone: &BTreeSet<u32>) -> Result<(), Problem> {
    delete_inner(doc, page, paths, gone, true)
}

fn delete_inner(doc: &mut Document, page: u32, mut paths: Vec<Vec<u32>>, gone: &BTreeSet<u32>, callouts: bool) -> Result<(), Problem> {
    let before = doc.clone();
    let joined: BTreeSet<u32>;
    let mut gone = gone;
    if callouts {
        let extra = super::callouts::delete_extra(doc, page, &paths, gone)?;
        // A callout on a locked layer is not deleted with the shape it is tied to.
        let going = extra.paths.iter().filter_map(|path| doc.shape(&ShapeRef { owner: Owner::Page(page), path: path.clone() }).map(|s| (s.id, &**s)));
        super::layers::refuse_locked_followers(doc, page, going, "deleted")?;
        if !extra.paths.is_empty() {
            joined = gone.iter().copied().chain(extra.gone.iter().copied()).collect();
            gone = &joined;
        }
        paths.extend(extra.paths);
        if !extra.writes.is_empty() {
            write_cells(doc, extra.writes)?;
        }
    }
    // **A `Connect` row, a glued end and a hold name a shape by its `ID`, and an `ID` finds the first shape that has it** (document order): on a page that
    // repeats an `ID` (a file may keep a deleted shape beside a live one of its `ID`), an `ID` of the shapes taken out whose first shape stays
    // names that shape, and what names it is left as it is. One walk of the page.
    let named = named_by(doc, page, &paths, gone)?;
    // The comments on the shapes that go go with them, and the replies to those: a drawing with none is not touched.
    if !doc.comments.is_empty() {
        doc.comments.remove_shapes(page, &named);
    }
    let unglued = unglue_connectors(doc, page, &named)?;
    // A shape held to a shape that goes (a guide, or whatever a file's alignment cells name) stays where it is, released, so that no formula names it
    // Its rows go below.
    let (named_ids, gone_ids): (HashSet<u32>, HashSet<u32>) = (named.iter().copied().collect(), gone.iter().copied().collect());
    crate::snap::hold::release_holders(doc, page, &named_ids, &gone_ids)?;
    if let Some(p) = page_mut(doc, page) {
        let kept = p.connects.iter().filter(|c| !named.contains(&c.to_sheet) && !named.contains(&c.from_sheet)).cloned().collect();
        p.connects = diacraft_doc::Connects::from_vec(kept);
    }
    // Later siblings and deeper shapes first, so that a removal does not move one still to be removed.
    paths.sort_unstable_by(|a, b| b.cmp(a));
    paths.dedup();
    for path in paths {
        remove_at(doc, page, &path);
    }
    settle(doc, &before, page, &[], &unglued)?;
    // A connector that went takes its jumps with it: the connectors it crossed are plain where it was.
    let removed: Vec<u32> = gone.iter().copied().collect();
    jumps::refresh(doc, &before, page, &removed, false)
}

/// The `ID`s of `gone` (those of the shapes at `paths` and of everything inside them) whose first shape on the page, in document order, is one of them: what
/// a `Connect` row or a reference that names the `ID` finds. On a page whose `ID`s are all different that is every one of `gone`.
fn named_by(doc: &Document, page: u32, paths: &[Vec<u32>], gone: &BTreeSet<u32>) -> Result<BTreeSet<u32>, Problem> {
    let wanted: HashSet<u32> = gone.iter().copied().collect();
    let found = scan::scan(doc, page, &wanted)?;
    // A shape is one of those taken out when a path of theirs is the start of its own: its depth's worth of lookups, whatever the number of paths.
    let taken: HashSet<&[u32]> = paths.iter().map(Vec::as_slice).collect();
    let inside = |at: &ShapeRef| (1..=at.path.len()).any(|k| at.path.get(..k).is_some_and(|start| taken.contains(start)));
    Ok(gone.iter().copied().filter(|id| found.found.get(id).is_some_and(inside)).collect())
}

/// The connectors outside `gone` glued to a shape inside it, each end made a plain point where it is. → their `ID`s.
///
/// The connectors are found in **one walk of the page** (`scan::scan`), not by a search from the top of the page for each `Connect` row, which was
/// the product of the rows and the shapes (500,000 connectors glued to one group is about 1e11 visits).
pub(super) fn unglue_connectors(doc: &mut Document, page: u32, gone: &BTreeSet<u32>) -> Result<Vec<u32>, Problem> {
    let Some(p) = doc.page(page) else { return Ok(Vec::new()) };
    let rows: Vec<(&diacraft_doc::Connect, (&'static str, &'static str))> = p
        .connects
        .iter()
        .filter(|c| gone.contains(&c.to_sheet) && !gone.contains(&c.from_sheet))
        .filter_map(|c| Some((c, end_cells(c.from_cell.as_deref())?)))
        .collect();
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: HashSet<u32> = rows.iter().map(|(c, _)| c.from_sheet).collect();
    let found = scan::scan(doc, page, &wanted)?;
    let mut writes: Vec<(SheetRef, CellAddr, CellEdit)> = Vec::with_capacity(rows.len() * 2);
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    {
        let reader = Reader::new(doc);
        for (c, (x_cell, y_cell)) in rows {
            let Some(at) = found.found.get(&c.from_sheet) else { continue };
            let sheet = SheetRef::Shape(at.clone());
            for name in [x_cell, y_cell] {
                let value = reader.number(&sheet, name);
                writes.push((
                    sheet.clone(),
                    CellAddr::direct(name),
                    CellEdit::Cell(Cell::constant(Value::from_text(&stencil::number_text(value))).with_unit("IN")),
                ));
            }
            ids.insert(c.from_sheet);
        }
    }
    write_cells(doc, writes)?;
    Ok(ids.into_iter().collect())
}

/// The two cells of the end a `Connect` row's `FromCell` names.
fn end_cells(from_cell: Option<&str>) -> Option<(&'static str, &'static str)> {
    match from_cell {
        Some("BeginX") | Some("BeginY") => Some(("BeginX", "BeginY")),
        Some("EndX") | Some("EndY") => Some(("EndX", "EndY")),
        _ => None,
    }
}

/// Takes the shape at `path` out of its parent.
fn remove_at(doc: &mut Document, page: u32, path: &[u32]) {
    let Some((last, parent)) = path.split_last() else { return };
    let Ok(last) = usize::try_from(*last) else { return };
    if parent.is_empty() {
        if let Some(p) = page_mut(doc, page) {
            p.shapes.remove(last);
        }
    } else if let Some(shape) = shape_mut(doc, page, parent) {
        shape.children.remove(last);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use diacraft_doc::{CellName, Connect, Connects, Sheet};

    use super::super::read::FINDS;
    use super::*;

    /// A page holding a box (`ID` 1) and `n` connectors (`ID` 2 up) glued at their begin to it, each by a `Connect` row, with plain ends.
    fn document_with_connectors_glued_to_a_box(n: u32) -> Document {
        let number = |text: &str| Cell::constant(Value::from_text(text));
        let mut doc = Document::new_blank();
        let mut shapes: Vec<Arc<Shape>> = vec![Arc::new(Shape::new(1))];
        let mut connects: Vec<Connect> = Vec::with_capacity(n as usize);
        for i in 0..n {
            let mut sheet = Sheet::default();
            for (name, value) in [("BeginX", "1"), ("BeginY", "2"), ("EndX", "3"), ("EndY", "4")] {
                sheet.cells.insert(CellName::new(name), number(value));
            }
            let mut shape = Shape::new(i + 2);
            shape.sheet = Arc::new(sheet);
            shapes.push(Arc::new(shape));
            connects.push(Connect {
                from_sheet: i + 2,
                from_cell: Some(Arc::from("BeginX")),
                from_part: Some(9),
                to_sheet: 1,
                to_cell: Some(Arc::from("PinX")),
                to_part: Some(3),
                ..Connect::default()
            });
        }
        if let Some(page) = page_mut(&mut doc, 1) {
            page.shapes = diacraft_doc::ShapeList::from_vec(shapes);
            page.connects = Connects::from_vec(connects);
        }
        doc
    }

    #[test]
    fn the_connectors_glued_to_a_shape_that_goes_are_found_in_one_walk_of_the_page_and_not_one_search_each() {
        // 100,000 connectors glued to one shape: a search from the top of the page for each row is about 5e9 visits, which takes tens of seconds.
        let n = 100_000_u32;
        let mut doc = document_with_connectors_glued_to_a_box(n);
        FINDS.with(|f| f.set(0));
        let started = std::time::Instant::now();
        let gone: BTreeSet<u32> = [1].into_iter().collect();
        let ids = unglue_connectors(&mut doc, 1, &gone).unwrap();
        let took = started.elapsed();
        eprintln!("unglueing {n} connectors on a page of {n}: {took:?}");
        assert_eq!(FINDS.with(|f| f.get()), 0, "no connector was looked for by itself");
        assert_eq!(ids.len(), n as usize);
        // A hang guard and no measure of cost: the count above (no search for a connector) is the claim.
        assert!(took < std::time::Duration::from_secs(60), "{took:?}");
        // Each begin is the constant of the value it held, with the unit a plain point has.
        let at = doc.find_shape(diacraft_doc::Owner::Page(1), 50_000).unwrap();
        let shape = doc.shape(&at).unwrap();
        let begin = shape.sheet.cell("BeginX").unwrap();
        assert_eq!((begin.formula.text(), begin.value.as_ref().map(|v| v.text()), begin.unit.as_deref()), (None, Some("1"), Some("IN")));
    }

    #[test]
    fn a_connect_row_that_names_no_end_cell_leaves_its_connector_alone() {
        let mut doc = document_with_connectors_glued_to_a_box(3);
        if let Some(page) = page_mut(&mut doc, 1) {
            let rows: Vec<Connect> = page.connects.iter().cloned().map(|c| Connect { from_cell: Some(Arc::from("PinX")), ..c }).collect();
            page.connects = Connects::from_vec(rows);
        }
        let before = doc.clone();
        let gone: BTreeSet<u32> = [1].into_iter().collect();
        assert!(unglue_connectors(&mut doc, 1, &gone).unwrap().is_empty());
        assert_eq!(doc, before);
    }
}
