//! Copy, cut, paste and duplicate.
//!
//! **The clipboard** is the session's (`Session::clipboard`), one for a document: it holds the shapes copied and the masters they use. A paste
//! is within one document (M3 second slice), so the masters a pasted shape uses are the document's already and are used by `ID`, never
//! copied: the clipboard keeps the masters only so that a paste after an undo that took a master away (a dropped shape's, with the drop) puts it back,
//! and a master the document has under the same `UniqueID` is that master, as is a built-in master the document holds under the same `BaseID` (a build before a change gave a built-in master
//! a `UniqueID` of the `BaseID`'s text, and this one gives it another: the `BaseID` is what the two have in common). The system clipboard is the window's business.
//!
//! Our rule. *Copy* takes the shapes named (a shape named with a group it is in is left to the group), each with everything inside it, in
//! the order of the drawing. A connector copied keeps its glue to a shape copied with it; a glue to a shape not copied is left out, and that end
//! is a plain point where it was (as the delete rule leaves the end of a connector whose shape is deleted). A shape that is a member of a group is
//! copied alone as a shape of the page: its cells are carried out through its groups' poses when it is pasted, so it draws as
//! it did. *Cut* is copy and delete in one step. *Paste* gives every shape copied, members included, a new `ID` above every `ID` of the page
//! and every one the session has given, in the order of the copy; puts the shapes on top of the page's drawing order, in their order;
//! and puts `Connect` rows between the copies where there were rows between the shapes. A formula in a copy that names a shape copied names its
//! copy ([`super::remap`]). A name that ends in the shape's `ID` ends in the copy's, and `UniqueID` is not copied, since it names
//! one shape. Text, fields, runs, formats, styles, data and pictures are the shape's own cells and parts and copy with it. A paste does not route
//! a connector again: the copies are the originals moved together, and a route is rows in the connector's own frame.
//!
//! Our rule. `edit.paste` with no place puts the copies one step (a quarter of an inch) right and down of the originals on the
//! page they were copied from, and one more for each paste of the same copy; on another page the first paste is at the originals' place and each
//! later one a step on. With a place, the middle of the copies' joint box lands there. `shape.duplicate` offsets by the distance it is given
//! (the same step by default) and leaves the clipboard alone.
//!
//! **Limits.** At most [`MAX_COPY_SHAPES`] shapes in one copy, members counted. A picture's relationship belongs to its page (`ForeignData`'s
//! `Rel` is an `r:id` of the page part's relationships), so a shape with a picture is pasted on the page it was copied from only.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, Connect, Document, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Master, Owner, RawAttrs, Shape, ShapeList, ShapeRef, SheetRef,
};

use super::arrange::{Rect, Target, page_points};
use super::group::new_id;
use super::lift::{Affine, Lift, POSE_CELLS, frame_cells, lift};
use super::read::Reader;
use super::remap::{freeze_refs, remap_sheet, renamed};
use super::scan;
use super::settle::settle_kept;
use super::shapes::delete_paths_with_callouts;
use super::{Problem, checked, page_mut};

/// The most shapes, members counted, one copy holds.
pub const MAX_COPY_SHAPES: usize = 50_000;

/// A shape on the clipboard: the shape with everything inside it, and, for a member of a group copied alone, the map that carries its cells out
/// of its groups and the `ID`s of the groups it leaves.
#[derive(Debug)]
struct Entry {
    shape: Arc<Shape>,
    lifted_by: Option<Affine>,
    /// The `ID`s of the groups the shape was in, outermost first: empty for a shape of the page. A formula in the copy that names one of them is
    /// made the constant of its cached value, as an ungroup makes it: the copy is not in that group, which may be
    /// taken apart or deleted, and a name of it would then be an error.
    leaves: Vec<u32>,
}

/// The `ID`s of the groups above the shape at `at`, outermost first: one walk down its path, no deeper than the model allows.
fn groups_above(doc: &Document, at: &ShapeRef) -> Vec<u32> {
    let mut found = Vec::new();
    let Owner::Page(page) = at.owner else { return found };
    let Some(page) = doc.page(page) else { return found };
    let mut list = &page.shapes;
    for step in at.path.iter().take(at.path.len().saturating_sub(1)).take(MAX_SHAPE_DEPTH) {
        let Some(group) = usize::try_from(*step).ok().and_then(|place| list.get(place)) else { break };
        found.push(group.id);
        list = &group.children;
    }
    found
}

#[derive(Debug)]
struct Contents {
    /// The shapes copied, in the order of the drawing.
    entries: Vec<Entry>,
    /// The `Connect` rows from any shape copied (with the `ID`s the shapes had): a paste keeps the ones whose two shapes are copies.
    connects: Vec<Connect>,
    /// The masters the shapes use, as the document had them.
    masters: Vec<Arc<Master>>,
    /// The page the shapes were on, and the box that holds them on it (left, bottom, right, top).
    page: u32,
    bounds: Rect,
    /// How many shapes, members counted.
    shapes: usize,
    /// Whether a shape copied has a picture, which belongs to its page.
    pictures: bool,
}

/// What `edit.copy` puts on the session's clipboard. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Clipboard {
    contents: Arc<Contents>,
    pastes: u32,
}

impl Clipboard {
    /// How many shapes were copied, not counting their members.
    pub fn len(&self) -> usize {
        self.contents.entries.len()
    }

    /// Whether nothing was copied.
    pub fn is_empty(&self) -> bool {
        self.contents.entries.is_empty()
    }

    /// How many shapes in all, members counted: how many a paste makes.
    pub fn shape_count(&self) -> usize {
        self.contents.shapes
    }

    /// The page the shapes were copied from.
    pub fn source_page(&self) -> u32 {
        self.contents.page
    }

    /// The box that holds the shapes on that page, in its drawing units: (left, bottom, right, top).
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        let r = self.contents.bounds;
        (r.x0, r.y0, r.x1, r.y1)
    }

    /// How many times this copy has been pasted.
    pub fn pastes(&self) -> u32 {
        self.pastes
    }

    /// The same copy, pasted once more.
    pub(crate) fn pasted_once_more(&self) -> Clipboard {
        Clipboard { contents: Arc::clone(&self.contents), pastes: self.pastes.saturating_add(1) }
    }
}

/// What a copy found, for `edit.copy` and `edit.cut`.
struct Copied {
    clipboard: Clipboard,
    /// The paths of the shapes copied, and the `ID`s of them and of everything inside them: what a cut takes out.
    paths: Vec<Vec<u32>>,
    gone: BTreeSet<u32>,
}

/// Calls `f` with `shape` and each shape inside it, depth first in document order (the order a paste gives `ID`s in). Iterative, and no deeper
/// than the model allows.
fn each_shape<'a>(shape: &'a Shape, mut f: impl FnMut(&'a Shape) -> Result<(), Problem>) -> Result<(), Problem> {
    f(shape)?;
    let mut stack: Vec<(&'a ShapeList, usize)> = vec![(&shape.children, 0)];
    while let Some(frame) = stack.last_mut() {
        let (list, at) = *frame;
        let Some(member) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        f(member)?;
        if stack.len() < MAX_SHAPE_DEPTH && !member.children.is_empty() {
            stack.push((&member.children, 0));
        }
    }
    Ok(())
}

/// The map from the frame the cells of a shape at `at` are in (its parent's) to the page: the poses of its groups, the innermost first.
fn carried_to_page(reader: &Reader<'_>, at: &ShapeRef) -> Affine {
    let mut map = Affine::translation(0.0, 0.0);
    for k in (1..at.path.len()).rev() {
        if let Some(prefix) = at.path.get(..k) {
            let group = ShapeRef { owner: at.owner, path: prefix.to_vec() };
            map = map.then(&Affine::of_pose(&reader.pose(&SheetRef::Shape(group))));
        }
    }
    map
}

fn copy_inner(doc: &Document, page: u32, ids: &[u32]) -> Result<Copied, Problem> {
    // A container is copied with what it holds.
    let ids = &super::container::with_contents(doc, page, ids)?;
    let (_, shapes) = scan::named(doc, page, ids)?;
    let shapes = scan::outermost(shapes);
    // The callouts tied to the shapes copied are copied with them (`callouts.rs`).
    let shapes = super::callouts::with_callouts(doc, page, shapes)?;
    let reader = Reader::new(doc);
    let mut entries: Vec<Entry> = Vec::with_capacity(shapes.len());
    let mut copied_ids: HashSet<u32> = HashSet::new();
    let mut master_ids: BTreeSet<u32> = BTreeSet::new();
    let (mut count, mut pictures) = (0_usize, false);
    for (id, at) in &shapes {
        let shape = doc.shape(at).ok_or_else(|| Problem::failed("the shape went away"))?;
        let member = at.path.len() > 1;
        if member && shape.master.is_none() && shape.master_shape.is_some() {
            return Err(Problem::failed(format!(
                "shape {id} is a member of a group made from a master and takes its cells from it: it is copied with its group"
            )));
        }
        let lifted_by = if member { Some(carried_to_page(&reader, at)) } else { None };
        if lifted_by.is_some_and(|map| map.reflects()) && reader.is_one_d(&SheetRef::Shape(at.clone())) {
            return Err(Problem::failed(format!("shape {id} is a connector in a flipped group: it is copied with its group")));
        }
        each_shape(shape, |s| {
            count = count.saturating_add(1);
            if count > MAX_COPY_SHAPES {
                return Err(Problem::failed(format!("a copy holds at most {MAX_COPY_SHAPES} shapes")));
            }
            copied_ids.insert(s.id);
            master_ids.extend(s.master);
            pictures |= s.foreign.as_ref().is_some_and(|f| f.rel_id.is_some());
            Ok(())
        })?;
        entries.push(Entry { shape: Arc::clone(shape), lifted_by, leaves: groups_above(doc, at) });
    }
    let bounds = {
        let boxes = shapes.iter().flat_map(|(id, at)| {
            let sheet = SheetRef::Shape(at.clone());
            page_points(&reader, &Target { id: *id, at: at.clone(), one_d: reader.is_one_d(&sheet), sheet })
        });
        Rect::of(boxes)?
    };
    let connects: Vec<Connect> =
        doc.page(page).map(|p| p.connects.iter().filter(|c| copied_ids.contains(&c.from_sheet)).cloned().collect()).unwrap_or_default();
    let masters: Vec<Arc<Master>> = master_ids.iter().filter_map(|id| doc.master(*id).cloned()).collect();
    let paths = shapes.iter().map(|(_, at)| at.path.clone()).collect();
    let gone: BTreeSet<u32> = copied_ids.iter().copied().collect();
    let contents = Contents { entries, connects, masters, page, bounds, shapes: count, pictures };
    Ok(Copied { clipboard: Clipboard { contents: Arc::new(contents), pastes: 0 }, paths, gone })
}

/// Copies shapes. The document is not touched.
pub fn copy_shapes(doc: &Document, page: u32, ids: &[u32]) -> Result<Clipboard, Problem> {
    Ok(copy_inner(doc, page, ids)?.clipboard)
}

/// Copies shapes and takes them out.
pub fn cut_shapes(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Clipboard, Problem> {
    let copied = copy_inner(doc, page, ids)?;
    // What a container holds goes with it, and so does a callout tied to a shape cut: one on a locked layer is not cut.
    let named: HashSet<u32> = ids.iter().copied().collect();
    let going = copied
        .paths
        .iter()
        .filter_map(|path| doc.shape(&ShapeRef { owner: Owner::Page(page), path: path.clone() }))
        .filter(|s| !named.contains(&s.id))
        .map(|s| (s.id, &**s));
    super::layers::refuse_locked_followers(doc, page, going, "cut")?;
    // A group that holds a shape on a locked layer is not cut.
    let taken: Vec<u32> =
        copied.paths.iter().filter_map(|path| doc.shape(&ShapeRef { owner: Owner::Page(page), path: path.clone() }).map(|s| s.id)).collect();
    super::layers::refuse_locked_in_group(doc, page, &taken, "cut")?;
    // The containers and the lists that held what is cut are brought to their setting after it, as a delete does (the lanes after a lane cut close the gap).
    let gone: Vec<u32> = copied.gone.iter().copied().collect();
    let holders = super::container::holding(doc, page, &gone)?;
    delete_paths_with_callouts(doc, page, copied.paths, &copied.gone)?;
    // No container or member keeps a tie to a shape that is gone.
    super::container::release(doc, page, &copied.gone)?;
    super::container::fit_after(doc, page, &holders)?;
    Ok(copied.clipboard)
}

// ---- paste -----------------------------------------------------------------------------------------------------------------------

/// Where a paste puts the copies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Each moved by this distance (drawing units) from where it was.
    Offset(f64, f64),
    /// The middle of the box that holds them at this place on the page.
    CenterAt(f64, f64),
}

/// What a paste made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pasted {
    /// The `ID`s of the shapes pasted, not counting their members, in the order of the drawing.
    pub ids: Vec<u32>,
    /// How many shapes were made, members counted.
    pub created: usize,
    /// The highest `ID` given.
    pub highest: u32,
    /// What the paste says beside what it made: the references a master it put back held to parts of its file, left out (`stencil::refs::leave_package_for_another`). Empty mostly.
    pub notes: Vec<String>,
}

/// The key a built-in master is known by in every build: its `BaseID`, in capitals, when it is the `BaseID` of a master of the built-in stencils (`stencil::is_builtin_guid`). A master of a file, whose
/// `BaseID` may be on masters that differ (copies keep it), has none: it is matched by its `UniqueID` alone.
fn built_in_key(m: &Master) -> Option<String> {
    let base = m.raw.get("BaseID")?;
    crate::stencil::is_builtin_guid(base).then(|| base.trim().to_ascii_uppercase())
}

/// The masters a paste uses, by the `ID` they had on the clipboard: the document's own master with that `ID` when it is the same master (same
/// `UniqueID`, or the same built-in `BaseID`, or the very same one), else a master of the document with the same `UniqueID` or the same built-in `BaseID`, else the clipboard's put back (under its `ID` when that is
/// free). Never a second copy of a master the document has.
/// → the map, and how many references a master put back had to parts its file had for it and the drawing's package does not (`stencil::refs::leave_package_for_another`), which were left out.
fn ensure_masters(doc: &mut Document, held: &[Arc<Master>]) -> Result<(HashMap<u32, u32>, usize), Problem> {
    let mut map: HashMap<u32, u32> = HashMap::new();
    let mut left_out = 0_usize;
    if held.is_empty() {
        return Ok((map, left_out));
    }
    // The document's masters, listed once (the first of an `ID` or a `UniqueID` is the one a search would find): a scan of them for each master held
    // is the product of the two, which a document of tens of thousands of masters and a copy that uses thousands of them makes large.
    let mut by_id: HashMap<u32, Arc<Master>> = HashMap::with_capacity(doc.masters.len());
    let mut by_unique: HashMap<Arc<str>, u32> = HashMap::new();
    let mut by_base: HashMap<String, u32> = HashMap::new();
    let mut highest = 0_u32;
    for d in doc.masters.iter() {
        by_id.entry(d.id).or_insert_with(|| Arc::clone(d));
        if let Some(unique) = &d.unique_id {
            by_unique.entry(Arc::clone(unique)).or_insert(d.id);
        }
        if let Some(base) = built_in_key(d) {
            by_base.entry(base).or_insert(d.id);
        }
        highest = highest.max(d.id);
    }
    for m in held {
        let held_base = built_in_key(m);
        let same = |d: &Arc<Master>| match (&d.unique_id, &m.unique_id) {
            (Some(a), Some(b)) if a == b => true,
            (None, None) => d.name_u == m.name_u,
            _ => held_base.is_some() && built_in_key(d) == held_base,
        };
        if by_id.get(&m.id).is_some_and(|d| Arc::ptr_eq(d, m) || same(d)) {
            map.insert(m.id, m.id);
        } else if let Some(found) =
            m.unique_id.as_ref().and_then(|unique| by_unique.get(unique)).or_else(|| held_base.as_ref().and_then(|base| by_base.get(base)))
        {
            map.insert(m.id, *found);
        } else {
            let id = if by_id.contains_key(&m.id) { highest.checked_add(1).ok_or_else(|| Problem::failed("no master ids are left"))? } else { m.id };
            let mut again = (**m).clone();
            again.id = id;
            // A master put back names no part of the package it was read from: the drawing's package gives it its own (two masters never name one part), with no rows, so a reference among
            // what it keeps beside its shapes would lead nowhere and is left out (the one step a master that leaves its package takes, shared with a bring).
            left_out = left_out.saturating_add(crate::stencil::refs::leave_package_for_another(&mut again));
            let again = Arc::new(again);
            doc.add_master((*again).clone())?;
            by_id.insert(id, Arc::clone(&again));
            if let Some(unique) = &again.unique_id {
                by_unique.entry(Arc::clone(unique)).or_insert(id);
            }
            if let Some(base) = built_in_key(&again) {
                by_base.entry(base).or_insert(id);
            }
            highest = highest.max(id);
            map.insert(m.id, id);
        }
    }
    Ok((map, left_out))
}

/// What copying a shape needs to know.
struct Ctx {
    /// The `ID` each shape copied has in its copy (the first, for an `ID` the shapes hold twice).
    ids: HashMap<u32, u32>,
    masters: HashMap<u32, u32>,
    /// The connectors copied whose glue to a shape not copied is left out: whether the begin, and whether the end, is a plain point now.
    unglue: HashMap<u32, (bool, bool)>,
    /// The `ID` the next shape copied gets.
    next: u32,
    /// Whether the copies go to another page than the shapes were on: a reference by `ID` to a shape not copied then reaches another shape.
    other_page: bool,
    /// The groups the entry being copied leaves ([`Entry::leaves`]): a formula that names one is made a constant.
    leaves: HashSet<u32>,
}

/// The sheet with the cells of the ends in `ends` made constants of the values they hold.
fn unglued(sheet: &Arc<diacraft_doc::Sheet>, ends: (bool, bool)) -> Arc<diacraft_doc::Sheet> {
    let mut next = (**sheet).clone();
    for (end, x_cell, y_cell) in [(false, "BeginX", "BeginY"), (true, "EndX", "EndY")] {
        if !(if end { ends.1 } else { ends.0 }) {
            continue;
        }
        for name in [x_cell, y_cell] {
            let Some(held) = sheet.cell(name) else { continue };
            let value = held.value.clone().unwrap_or_else(|| diacraft_doc::Value::from_text("0"));
            let mut cell = Cell::constant(value);
            cell.unit = held.unit.clone().or_else(|| Some(Arc::from("IN")));
            // The cell is there, so the sheet has room for it.
            if next.set_cell(&CellAddr::direct(name), cell).is_err() {
                return Arc::clone(sheet);
            }
        }
    }
    Arc::new(next)
}

/// `raw` without the attribute `name`.
fn without(raw: &RawAttrs, name: &str) -> RawAttrs {
    if raw.get(name).is_none() { raw.clone() } else { RawAttrs::from_pairs(raw.iter().filter(|(n, _)| *n != name)) }
}

/// The attributes of a shape's own that a copy does not keep: `UniqueID`, which names one shape of a VSDX file, and `xml:id` and `draw:id`, which
/// name one element of an `.odg` (a copy that held the id of the shape it was copied from would be a second element with it,
/// and a connector glued to the one would name two).
const NOT_COPIED: [&str; 3] = ["UniqueID", "xml:id", "draw:id"];

/// The attributes of `raw` a copy keeps.
fn copied_raw(raw: &RawAttrs) -> RawAttrs {
    NOT_COPIED.iter().fold(raw.clone(), |kept, name| without(&kept, name))
}

/// A copy of `shape` and everything inside it, with the `ID`s [`Ctx`] gives.
fn copy_shape(shape: &Shape, ctx: &mut Ctx, depth: usize) -> Result<Arc<Shape>, Problem> {
    if depth >= MAX_SHAPE_DEPTH {
        return Err(Problem::failed("shapes are nested too deeply"));
    }
    let id = ctx.next;
    ctx.next = ctx.next.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let mut copy = Shape { id, ..shape.clone() };
    // A name of a group the copy has left is its number first, before the names of the shapes copied are changed.
    // A tie to a callout or a target that is not copied is taken out of the copy.
    let own = super::callouts::cut_uncopied(shape.id, &shape.sheet, &ctx.ids)?;
    let source = if ctx.leaves.is_empty() { own } else { freeze_refs(&own, &ctx.leaves) };
    copy.sheet = remap_sheet(&source, &ctx.ids, ctx.other_page);
    if let Some(ends) = ctx.unglue.get(&shape.id) {
        copy.sheet = unglued(&copy.sheet, *ends);
    }
    for (kept, name) in [(&shape.name, &mut copy.name), (&shape.name_u, &mut copy.name_u)] {
        if let Some(new) = kept.as_deref().and_then(|n| renamed(n, shape.id, id)) {
            *name = Some(Arc::from(new));
        }
    }
    copy.raw = copied_raw(&shape.raw);
    // The attributes of an `.odg` shape's own that no field reads are in its `odf` slot, and the ids among them name one element as well: a copy that held them
    // would be a second element with the same `xml:id`. The rest of the slot goes with the copy, so a pasted shape keeps the values the file held for it.
    if shape.ext.get().is_some_and(|d| NOT_COPIED.iter().any(|name| d.odf.kept.raw.get(name).is_some())) {
        let data = copy.ext.make_mut();
        data.odf.kept.raw = copied_raw(&data.odf.kept.raw);
    }
    copy.master = shape.master.map(|m| ctx.masters.get(&m).copied().unwrap_or(m));
    if !shape.children.is_empty() {
        let members = shape.children.iter().map(|member| copy_shape(member, ctx, depth + 1)).collect::<Result<Vec<_>, _>>()?;
        copy.children = ShapeList::from_vec(members);
    }
    Ok(Arc::new(copy))
}

/// `connect` for the copies `from` and `to`.
fn connect_between(connect: &Connect, from: u32, to: u32) -> Connect {
    Connect { from_sheet: from, to_sheet: to, ..connect.clone() }
}

/// Which end of a connector a `Connect` row's `FromCell` is at: `false` the begin, `true` the end.
fn end_of(from_cell: Option<&str>) -> Option<bool> {
    match from_cell {
        Some("BeginX" | "BeginY") => Some(false),
        Some("EndX" | "EndY") => Some(true),
        _ => None,
    }
}

/// Pastes the clipboard on a page: new `ID`s above `above` and above every `ID` of the page.
pub fn paste_shapes(doc: &mut Document, page: u32, clip: &Clipboard, at: Placement, above: u32) -> Result<Pasted, Problem> {
    let c = &*clip.contents;
    let found = scan::scan(doc, page, &HashSet::new())?;
    if found.count.saturating_add(c.shapes) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    if c.pictures && page != c.page {
        return Err(Problem::failed("a picture belongs to the page it is on: a shape with one is pasted on the page it was copied from"));
    }
    let first = new_id(&found, above)?;
    let shift = match at {
        Placement::Offset(dx, dy) => (checked("dx", dx)?, checked("dy", dy)?),
        Placement::CenterAt(x, y) => {
            let middle = c.bounds.middle();
            (checked("x", x)? - middle.0, checked("y", y)? - middle.1)
        }
    };
    let (masters, left_out) = ensure_masters(doc, &c.masters)?;
    // The `ID`s, in the order the copies are made in.
    let mut ctx = Ctx { ids: HashMap::new(), masters, unglue: HashMap::new(), next: first, other_page: page != c.page, leaves: HashSet::new() };
    let mut top_ids: Vec<u32> = Vec::with_capacity(c.entries.len());
    let mut numbered = first;
    for entry in &c.entries {
        top_ids.push(numbered);
        each_shape(&entry.shape, |s| {
            ctx.ids.entry(s.id).or_insert(numbered);
            numbered = numbered.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
            Ok(())
        })?;
    }
    let highest = numbered.saturating_sub(1);
    // The glue that stays between the copies, and the ends left as plain points.
    let mut rows: Vec<Connect> = Vec::new();
    for connect in &c.connects {
        match (ctx.ids.get(&connect.from_sheet), ctx.ids.get(&connect.to_sheet)) {
            (Some(from), Some(to)) => rows.push(connect_between(connect, *from, *to)),
            (Some(_), None) => {
                if let Some(end) = end_of(connect.from_cell.as_deref()) {
                    let ends = ctx.unglue.entry(connect.from_sheet).or_default();
                    if end {
                        ends.1 = true;
                    } else {
                        ends.0 = true;
                    }
                }
            }
            _ => {}
        }
    }
    let glued: HashSet<(u32, bool)> = rows.iter().filter_map(|r| Some((r.from_sheet, end_of(r.from_cell.as_deref())?))).collect();
    let mut copies: Vec<Arc<Shape>> = Vec::with_capacity(c.entries.len());
    for entry in &c.entries {
        ctx.leaves = entry.leaves.iter().copied().collect();
        copies.push(copy_shape(&entry.shape, &mut ctx, 0)?);
    }
    // Onto the page: on top, in order.
    let start = doc.page(page).map_or(0, |p| p.shapes.len());
    {
        let p = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
        for copy in copies {
            p.push_shape(copy);
        }
        for row in &rows {
            p.connects.push(row.clone());
        }
    }
    // Every copy of a shape held to a guide is released, one put where its original is too.
    let copied: HashSet<u32> = ctx.ids.values().copied().collect();
    crate::snap::hold::release_shapes(doc, page, &copied)?;
    // The copies are moved, and a member copied alone is carried out of its groups: one map for each.
    let moved = Affine::translation(shift.0, shift.1);
    // The shapes to carry, and for each whether it came out of a group (its pose cells, and the cells of it that carry a point between frames,
    // are then worked out whatever was written).
    let mut items: Vec<Lift> = Vec::new();
    let mut from_a_group: Vec<bool> = Vec::new();
    for (k, entry) in c.entries.iter().enumerate() {
        let by = entry.lifted_by.map_or(moved, |map| map.then(&moved));
        if entry.lifted_by.is_some() || shift != (0.0, 0.0) {
            let path = u32::try_from(start + k).map_err(|_| Problem::failed("too many shapes"))?;
            items.push(Lift { id: top_ids.get(k).copied().unwrap_or(0), at: ShapeRef { owner: Owner::Page(page), path: vec![path] }, by });
            from_a_group.push(entry.lifted_by.is_some());
        }
    }
    let written = lift(doc, &items, &glued)?;
    let mut cells: Vec<(SheetRef, CellAddr)> = Vec::new();
    for (place, item) in items.iter().enumerate() {
        let sheet = SheetRef::Shape(item.at.clone());
        cells.extend(written.get(place).into_iter().flatten().map(|name| (sheet.clone(), CellAddr::direct(name))));
        if from_a_group.get(place).copied().unwrap_or(false) {
            cells.extend(POSE_CELLS.iter().map(|name| (sheet.clone(), CellAddr::direct(name))));
            if let Some(shape) = doc.shape(&item.at) {
                cells.extend(frame_cells(&shape.sheet).into_iter().map(|addr| (sheet.clone(), addr)));
            }
        }
    }
    settle_kept(doc, page, &diacraft_calc::Changes { cells, ..diacraft_calc::Changes::default() })?;
    // The copies of a container and its members have ties among themselves; a copy of a member alone has none to the container it was in, and joins the one it was put inside.
    let copies: HashSet<u32> = ctx.ids.values().copied().collect();
    // A loop among the copies is broken, and said.
    let untied = super::container::repair_copies(doc, page, &copies)?;
    let fitted = super::container::arrive(doc, page, &top_ids)?;
    let mut notes = if left_out > 0 { vec![crate::stencil::left_out_words(left_out, "its")] } else { Vec::new() };
    notes.extend(untied);
    // A container on a locked layer the copies were put in is passed over, and noted.
    notes.extend(super::container::locked_notes(doc, page, &fitted.locked)?);
    Ok(Pasted { ids: top_ids, created: c.shapes, highest, notes })
}

/// Copies shapes and pastes them on the same page at once, each `offset` away (drawing units): the clipboard is not used.
pub fn duplicate_shapes(doc: &mut Document, page: u32, ids: &[u32], offset: (f64, f64), above: u32) -> Result<Pasted, Problem> {
    let clip = copy_shapes(doc, page, ids)?;
    paste_shapes(doc, page, &clip, Placement::Offset(offset.0, offset.1), above)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn the_ends_of_a_connector_are_told_from_the_cell_a_connect_row_names() {
        assert_eq!(end_of(Some("BeginX")), Some(false));
        assert_eq!(end_of(Some("BeginY")), Some(false));
        assert_eq!(end_of(Some("EndX")), Some(true));
        assert_eq!(end_of(Some("EndY")), Some(true));
        assert_eq!(end_of(Some("PinX")), None);
        assert_eq!(end_of(None), None);
    }

    #[test]
    fn an_attribute_is_taken_out_of_a_list_and_the_others_stay_in_order() {
        let raw = RawAttrs::from_pairs([("OriginalID", "4"), ("UniqueID", "{1}"), ("IsCustomName", "1")]);
        let kept = without(&raw, "UniqueID");
        assert_eq!(kept.iter().collect::<Vec<_>>(), [("OriginalID", "4"), ("IsCustomName", "1")]);
        assert_eq!(without(&raw, "Nothing"), raw);
    }

    #[test]
    fn a_copy_keeps_no_id_of_the_shape_it_was_copied_from_and_every_other_attribute() {
        let raw = RawAttrs::from_pairs([("UniqueID", "{1}"), ("xml:id", "id3"), ("ext:note", "x"), ("draw:id", "id3"), ("draw:layer", "layout")]);
        assert_eq!(copied_raw(&raw).iter().collect::<Vec<_>>(), [("ext:note", "x"), ("draw:layer", "layout")]);
        for one in NOT_COPIED {
            let alone = RawAttrs::from_pairs([("ext:note", "x"), (one, "v")]);
            assert_eq!(copied_raw(&alone).iter().collect::<Vec<_>>(), [("ext:note", "x")], "{one}");
        }
        let plain = RawAttrs::from_pairs([("ext:note", "x")]);
        assert_eq!(copied_raw(&plain), plain);
    }

    #[test]
    fn a_clipboard_clone_shares_what_it_holds_and_counts_its_own_pastes() {
        let contents = Contents {
            entries: vec![],
            connects: vec![],
            masters: vec![],
            page: 1,
            bounds: Rect { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 },
            shapes: 0,
            pictures: false,
        };
        let clip = Clipboard { contents: Arc::new(contents), pastes: 0 };
        let again = clip.pasted_once_more();
        assert_eq!((clip.pastes(), again.pastes()), (0, 1));
        assert!(Arc::ptr_eq(&clip.contents, &again.contents));
        assert!(clip.is_empty());
        assert_eq!(clip.bounds(), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn the_groups_above_a_shape_are_found_in_one_walk_outermost_first_and_a_bad_path_finds_none() {
        use crate::draw::{drop_shape, group_shapes};
        let mut doc = Document::new_blank();
        let rectangle = crate::stencil::find_master("rectangle").unwrap();
        let ids: Vec<u32> = (0..3).map(|k| drop_shape(&mut doc, 1, rectangle, 1.0 + f64::from(k), 1.0).unwrap().id).collect();
        let inner = group_shapes(&mut doc, 1, ids.get(..2).unwrap(), 0).unwrap().id;
        let outer = group_shapes(&mut doc, 1, &[inner, ids[2]], 0).unwrap().id;
        let deep = doc.find_shape(Owner::Page(1), ids[0]).unwrap();
        assert_eq!(groups_above(&doc, &deep), [outer, inner]);
        let middle = doc.find_shape(Owner::Page(1), inner).unwrap();
        assert_eq!(groups_above(&doc, &middle), [outer]);
        let top = doc.find_shape(Owner::Page(1), outer).unwrap();
        assert!(groups_above(&doc, &top).is_empty(), "a shape of the page is in no group");
        // A path that leads nowhere, a page that is not there and a master's path give nothing and never panic.
        assert!(groups_above(&doc, &ShapeRef { owner: Owner::Page(1), path: vec![9, 9, 9] }).is_empty());
        assert!(groups_above(&doc, &ShapeRef { owner: Owner::Page(77), path: vec![0, 0] }).is_empty());
        assert!(groups_above(&doc, &ShapeRef { owner: Owner::Master(1), path: vec![0, 0] }).is_empty());
        assert!(groups_above(&doc, &ShapeRef { owner: Owner::Page(1), path: vec![] }).is_empty());
    }

    #[test]
    fn a_master_pasted_into_another_drawing_names_no_part_of_the_package_it_came_from() {
        // A master read from a file names the part it came from and the `Rel` of its row: the drawing it is pasted into has a package of its own, which names its parts, and
        // two masters would otherwise name one part of it.
        let mut master = Master {
            id: 3,
            name_u: Some(Arc::from("Box")),
            part_name: Some(Arc::from("/visio/masters/master1.xml")),
            rel_id: Some(Arc::from("rId1")),
            ..Master::default()
        };
        let kept = || diacraft_doc::Kept { raw: diacraft_doc::RawAttrs::from_pairs([("k", "v")]), ..Default::default() };
        master.ext.make_mut().vsdx.0.push((Arc::from("Rel"), kept()));
        // The root of its part held an attribute and a prefix: the prefix is kept for what is kept below it, the rest goes.
        let root = diacraft_doc::Kept { raw: diacraft_doc::RawAttrs::from_pairs([("k", "v"), ("xmlns:zz", "urn:zz")]), ..Default::default() };
        master.ext.make_mut().vsdx.0.push((Arc::from("MasterContents"), root));
        master.ext.make_mut().vsdx.0.push((Arc::from("Shapes"), kept()));
        master.ext.make_mut().vsdx.0.push((Arc::from("Connects"), kept()));
        master.ext.make_mut().vsdx.0.push((Arc::from("NoPageSheet"), diacraft_doc::Kept::default()));
        let mut doc = Document::new_blank();
        let (map, left_out) = ensure_masters(&mut doc, &[Arc::new(master)]).unwrap();
        assert_eq!(left_out, 0, "nothing it keeps is a reference");
        let copy = doc.master(*map.get(&3).unwrap()).unwrap();
        assert!(copy.part_name.is_none() && copy.rel_id.is_none(), "{:?} {:?}", copy.part_name, copy.rel_id);
        let keys: Vec<&str> = copy.ext.get().map(|e| e.vsdx.0.iter().map(|(k, _)| &**k).collect()).unwrap_or_default();
        // The nodes its `Shapes` and `Connects` hold and whether its row had a `PageSheet` are the master's own; of the root of its part, the prefixes it declared.
        assert_eq!(
            keys,
            ["MasterContents", "Shapes", "Connects", "NoPageSheet"],
            "what is the master's own stays; the row's `Rel` and the root of its part go"
        );
        let root = copy.ext.get().and_then(|e| e.vsdx.0.iter().find(|(k, _)| &**k == "MasterContents").map(|(_, v)| v.clone())).unwrap();
        assert_eq!(root.raw, diacraft_doc::RawAttrs::from_pairs([("xmlns:zz", "urn:zz")]), "the root keeps its prefixes and nothing else");
        assert!(root.kept.is_empty());
    }

    /// A master with the ids given.
    fn with_ids(id: u32, name: &str, unique: &str, base: &str) -> Arc<Master> {
        let mut m = Master { id, name_u: Some(Arc::from(name)), unique_id: Some(Arc::from(unique)), ..Master::default() };
        m.raw.push("BaseID", base);
        Arc::new(m)
    }

    #[test]
    fn a_built_in_master_held_with_the_unique_id_of_0_2_3_is_the_drawings_master_of_the_same_base_id() {
        // A build before this one gave a built-in master a `UniqueID` that is its `BaseID`'s text, and this one gives it another. The two are one master by the `BaseID`.
        let spec = crate::stencil::find_master("process").unwrap();
        let base = spec.guid();
        let mut doc = Document::new_blank();
        let have = crate::stencil::ensure_master(&mut doc, spec).unwrap();
        let held = [with_ids(have + 4, "Process", &base, &base)];
        let (map, _) = ensure_masters(&mut doc, &held).unwrap();
        assert_eq!(map.get(&(have + 4)), Some(&have), "the document's own master, by its BaseID");
        assert_eq!(doc.masters.len(), 1, "no second copy");
        // The same master held under the `ID` the document gives its own: the same master.
        let held = [with_ids(have, "Process", &base, &base)];
        let (map, _) = ensure_masters(&mut doc, &held).unwrap();
        assert_eq!((map.get(&have), doc.masters.len()), (Some(&have), 1));
        // A master of a file whose BaseID is no built-in's is matched by its `UniqueID` alone, as before.
        let other = [with_ids(9, "Process", "{F0000000-0000-4000-8000-000000000009}", "{F0000000-0000-4000-8000-00000000000A}")];
        let (map, _) = ensure_masters(&mut doc, &other).unwrap();
        assert_eq!((map.get(&9), doc.masters.len()), (Some(&9), 2), "put back: another master");
        let again = [with_ids(11, "Other", "{F0000000-0000-4000-8000-000000000009}", "{F0000000-0000-4000-8000-00000000000B}")];
        let (map, _) = ensure_masters(&mut doc, &again).unwrap();
        assert_eq!((map.get(&11), doc.masters.len()), (Some(&9), 2), "the same UniqueID is the same master");
    }

    #[test]
    fn two_masters_of_a_file_that_share_a_base_id_that_is_no_built_in_ones_stay_two() {
        let mut doc = Document::new_blank();
        let held = [
            with_ids(1, "A", "{A0000000-0000-4000-8000-000000000001}", "{B0000000-0000-4000-8000-000000000001}"),
            with_ids(2, "B", "{A0000000-0000-4000-8000-000000000002}", "{B0000000-0000-4000-8000-000000000001}"),
        ];
        let (map, _) = ensure_masters(&mut doc, &held).unwrap();
        assert_eq!((map.get(&1), map.get(&2), doc.masters.len()), (Some(&1), Some(&2), 2), "a copy that keeps a BaseID is another master");
    }
}
