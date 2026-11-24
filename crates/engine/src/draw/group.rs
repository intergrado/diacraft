//! Group and ungroup.
//!
//! (ours; a group is a `Shape` of `Type` `Group` whose `Shapes` hold its members; a member's cells are in the frame of its group).
//! `shape.group` makes a group whose frame is the box that holds the members, in the frame
//! they are in (the page's, or the group's they are in): its pin is the middle of the box, its `LocPinX` and `LocPinY` are half its `Width` and
//! `Height` by formula (`Width*0.5`), its angle is 0. The members' cells are carried into it ([`super::lift`]: a move by minus the
//! box's lower-left corner), so that each draws where it did. The group stands in the list where the topmost of its members stood, and the members
//! keep the order they had. The shapes named must be in one list (the page's, or one group's members); a shape named with a group it is in is left
//! to the group; two shapes are the fewest.
//!
//! `shape.ungroup` is the opposite for each group named: its members go to the list the group was in, in the place of the group, in their order,
//! and their cells are carried out through the group's pose, which may be turned and flipped. The group's own cells go with it. **A connector
//! glued to the group itself is left with that end a plain point where it was** (which this is for the group's shape). A group named
//! with a group it is in is not ungrouped in the same call.
//!
//! Neither command routes a connector again, and neither changes a Connect row: the `ID`s of the members stay, a glue is to a shape by its `ID`, and
//! a formula that reaches a member by `Sheet.<id>` or by `PAR(PNT(...))` finds it where it is.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use diacraft_calc::Changes;
use diacraft_doc::{
    Cell, CellAddr, CellName, Connects, Document, Links, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, Shape, ShapeList, ShapeRef, ShapeType, Sheet,
    SheetRef,
};

use super::arrange::{Rect, put_siblings, siblings};
use super::lift::{Affine, Lift, POSE_CELLS, frame_cells, lift};
use super::local::localise_geometry;
use super::nested::tie_to_group;
use super::read::Reader;
use super::remap::freeze_refs;
use super::route::P;
use super::scan::{self, Scan};
use super::settle::settle_kept;
use super::shapes::unglue_connectors;
use super::{Problem, checked, shape_mut};
use crate::stencil::number_text;

/// The deepest a group may make shapes nest: 90 levels. The model allows 200 (`MAX_SHAPE_DEPTH`), but the reader of a file takes XML 200 elements deep
/// and a level of group costs two (`Shape` and `Shapes`), so a drawing nested past about 98 levels could be saved and not read again.
pub const MAX_GROUP_DEPTH: usize = 90;

/// The most members one `shape.ungroup` takes apart, the members of every group named together: 50,000, as many as one copy holds. The ids a command
/// takes are capped, but one id may name a group of any number of members, and each member is carried, written and worked out.
pub const MAX_UNGROUP_MEMBERS: usize = 50_000;

// What the cells of a command cost to work out, for the tests that hold it to a count: one for each shape and cell it handles. Not kept outside tests.
#[cfg(test)]
thread_local! {
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n)));
}

#[cfg(not(test))]
fn note_work(_: usize) {}

/// What `shape.group` made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouped {
    /// The new group's `ID`.
    pub id: u32,
    /// How many shapes it holds directly.
    pub members: usize,
}

/// What `shape.ungroup` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ungrouped {
    /// How many groups were taken apart.
    pub groups: usize,
    /// The `ID`s of the shapes that were their members, in the order of the drawing.
    pub members: Vec<u32>,
}

/// Which ends of the connectors among `ids` are glued: `(ID, false)` for the begin and `(ID, true)` for the end. One pass over the page's `Connect`
/// rows.
pub(super) fn glued_ends(doc: &Document, page: u32, ids: &HashSet<u32>) -> HashSet<(u32, bool)> {
    let Some(p) = doc.page(page) else { return HashSet::new() };
    p.connects
        .iter()
        .filter(|c| ids.contains(&c.from_sheet))
        .filter_map(|c| match c.from_cell.as_deref() {
            Some("BeginX" | "BeginY") => Some((c.from_sheet, false)),
            Some("EndX" | "EndY") => Some((c.from_sheet, true)),
            _ => None,
        })
        .collect()
}

/// The points that make the box of a shape in its parent's frame: its four corners carried through its pose, or for a one-dimensional shape its two ends and
/// **nothing else** (the box a `.odg` reader makes of a group counts a line by its two ends, and the writer holds a new group to that
/// box; a route that leaves the box of the ends is no part of the group's frame, so that the session's frame and the reopen's are one).
pub(super) fn parent_points(reader: &Reader<'_>, at: &ShapeRef) -> Vec<P> {
    let sheet = SheetRef::Shape(at.clone());
    if reader.is_one_d(&sheet) {
        let end = |x: &str, y: &str| (reader.number(&sheet, x), reader.number(&sheet, y));
        vec![end("BeginX", "BeginY"), end("EndX", "EndY")]
    } else {
        let pose = reader.pose(&sheet);
        let (w, h) = (reader.number(&sheet, "Width"), reader.number(&sheet, "Height"));
        [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|p| pose.up(p)).to_vec()
    }
}

/// The group shape: a pin at the middle of its box, the box's size, and a local pin at the middle by formula. Linked to the root style, as a
/// connector is, so that the group properties it reads (`DisplayMode`, `SelectMode`) are the root style's.
fn group_shape(id: u32, rect: &Rect, members: Vec<Arc<Shape>>) -> Shape {
    let (width, height) = (rect.x1 - rect.x0, rect.y1 - rect.y0);
    let middle = rect.middle();
    let length = |v: f64| Cell::constant(diacraft_doc::Value::from_text(&number_text(v))).with_unit("IN");
    let half = |source: &str, v: f64| Cell::with_formula(source, Some(diacraft_doc::Value::from_text(&number_text(v)))).with_unit("IN");
    let mut sheet = Sheet { links: Links { line: Some(0), fill: Some(0), text: Some(0) }, ..Sheet::default() };
    for (name, cell) in [
        ("PinX", length(middle.0)),
        ("PinY", length(middle.1)),
        ("Width", length(width)),
        ("Height", length(height)),
        ("LocPinX", half("Width*0.5", width / 2.0)),
        ("LocPinY", half("Height*0.5", height / 2.0)),
    ] {
        sheet.cells.insert(CellName::new(name), cell);
    }
    Shape { kind: ShapeType::Group, sheet: Arc::new(sheet), children: ShapeList::from_vec(members), ..Shape::new(id) }
}

/// The cells to work out after shapes have a new parent: the pose cells of each, the cells it wrote, and the cells of its own that carry a point
/// between frames. `shapes` is each shape's `ID` and where it is now, and `written` the names of the cells written under each, in the same order
/// ([`lift`] gives them so): one pass, linear in the shapes and the cells.
pub(super) fn reparent_changes(doc: &Document, shapes: &[(u32, ShapeRef)], written: &[Vec<&'static str>]) -> Vec<(SheetRef, CellAddr)> {
    let mut cells: Vec<(SheetRef, CellAddr)> = Vec::new();
    for ((_, at), names) in shapes.iter().zip(written) {
        note_work(1 + names.len());
        let sheet = SheetRef::Shape(at.clone());
        cells.extend(POSE_CELLS.iter().map(|name| (sheet.clone(), CellAddr::direct(name))));
        cells.extend(names.iter().map(|name| (sheet.clone(), CellAddr::direct(name))));
        if let Some(shape) = doc.shape(at) {
            cells.extend(frame_cells(&shape.sheet).into_iter().map(|addr| (sheet.clone(), addr)));
        }
    }
    cells
}

/// How many levels of shapes `shape` holds, itself counted: 1 for a shape with no members. Iterative, and no deeper than the model allows.
pub(super) fn levels(shape: &Shape) -> usize {
    let mut deepest = 1_usize;
    // The list being walked, the place of the next shape in it, and the level its shapes are at.
    let mut stack: Vec<(&ShapeList, usize, usize)> = vec![(&shape.children, 0, 2)];
    while let Some(frame) = stack.last_mut() {
        let (list, at, level) = *frame;
        let Some(member) = list.get(at) else {
            stack.pop();
            continue;
        };
        frame.1 = at.saturating_add(1);
        deepest = deepest.max(level);
        if level < MAX_SHAPE_DEPTH && !member.children.is_empty() {
            stack.push((&member.children, 0, level + 1));
        }
    }
    deepest
}

// ---- group -----------------------------------------------------------------------------------------------------------------------

/// Makes a group of the shapes named, with an `ID` above `above` and above every `ID` on the page.
pub fn group_shapes(doc: &mut Document, page: u32, ids: &[u32], above: u32) -> Result<Grouped, Problem> {
    let (scan, shapes) = scan::named(doc, page, ids)?;
    let shapes = scan::outermost(shapes);
    if shapes.len() < 2 {
        return Err(Problem::bad("a group needs two shapes at least (a shape named with the group it is in counts once)"));
    }
    let parent: Vec<u32> = shapes.first().and_then(|(_, at)| at.path.split_last().map(|(_, up)| up.to_vec())).unwrap_or_default();
    if shapes.iter().any(|(_, at)| at.path.split_last().map(|(_, up)| up) != Some(parent.as_slice())) {
        return Err(Problem::failed("shapes that are in different groups cannot be grouped together yet"));
    }
    let id = new_id(&scan, above)?;
    // The group is one level deeper than the members were, and a file holds no shapes nested deeper than the model allows.
    let deepest = shapes.iter().filter_map(|(_, at)| doc.shape(at)).map(|shape| levels(shape)).max().unwrap_or(1);
    if parent.len().saturating_add(1).saturating_add(deepest) > MAX_GROUP_DEPTH {
        return Err(Problem::failed(format!("shapes are nested too deeply to group (at most {MAX_GROUP_DEPTH} levels)")));
    }
    let member_ids: HashSet<u32> = shapes.iter().map(|(id, _)| *id).collect();
    let rect = {
        let reader = Reader::new(doc);
        Rect::of(shapes.iter().flat_map(|(_, at)| parent_points(&reader, at)))?
    };
    for v in [rect.x0, rect.y0, rect.x1, rect.y1] {
        checked("the box of the group", v)?;
    }
    // A guide grouped is in the group's frame, where the formulas of the shapes held to it would read its pin: they are released first, while its pin is
    // the page's, and stay where they are.
    let guides: HashSet<u32> =
        shapes.iter().filter(|(_, at)| doc.shape(at).is_some_and(|s| s.kind == diacraft_doc::ShapeType::Guide)).map(|(id, _)| *id).collect();
    crate::snap::hold::release_holders(doc, page, &guides, &HashSet::new())?;
    // The members' cells go into the group's frame, whose origin is the box's lower-left corner.
    let items: Vec<Lift> = shapes.iter().map(|(id, at)| Lift { id: *id, at: at.clone(), by: Affine::translation(-rect.x0, -rect.y0) }).collect();
    let written = lift(doc, &items, &glued_ends(doc, page, &member_ids))?;
    // The group takes the place of the topmost member.
    let list = siblings(doc, page, &parent).ok_or_else(|| Problem::failed("the list of shapes went away"))?;
    let picked: BTreeSet<usize> = shapes.iter().filter_map(|(_, at)| at.path.last().and_then(|i| usize::try_from(*i).ok())).collect();
    let top = picked.last().copied().unwrap_or(0);
    let members: Vec<Arc<Shape>> = list.iter().enumerate().filter(|(i, _)| picked.contains(i)).map(|(_, s)| Arc::clone(s)).collect();
    let group = Arc::new(group_shape(id, &rect, members));
    let mut rebuilt: Vec<Arc<Shape>> = Vec::with_capacity(list.len());
    let mut group_at = 0_usize;
    for (i, shape) in list.iter().enumerate() {
        if i == top {
            group_at = rebuilt.len();
            rebuilt.push(Arc::clone(&group));
        }
        if !picked.contains(&i) {
            rebuilt.push(Arc::clone(shape));
        }
    }
    put_siblings(doc, page, &parent, ShapeList::from_vec(rebuilt))?;
    // The cells to work out: the group's own, and what moved into it.
    let mut path = parent.clone();
    path.push(u32::try_from(group_at).map_err(|_| Problem::failed("too many shapes"))?);
    let group_ref = ShapeRef { owner: Owner::Page(page), path };
    let moved: Vec<(u32, ShapeRef)> =
        (0..shapes.len()).map(|k| (shapes.get(k).map_or(0, |(id, _)| *id), group_ref.child(u32::try_from(k).unwrap_or(u32::MAX)))).collect();
    // An instance that becomes a member carries its master's geometry itself (kept by so that a reader that does not know
    // the master draws it) and holds its size and its pin as formulas of the group, as a file's nested instance does. The formulas keep
    // the numbers the cells held, so nothing that reads them is worked out again.
    let members: Vec<ShapeRef> = moved.iter().map(|(_, at)| at.clone()).collect();
    localise_geometry(doc, page, &members)?;
    let came_from = ShapeRef { owner: Owner::Page(page), path: parent.clone() };
    let old_parent = if parent.is_empty() { None } else { doc.shape(&came_from).map(|shape| shape.id) };
    tie_to_group(doc, id, &group_ref, &members, old_parent)?;
    let mut cells = reparent_changes(doc, &moved, &written);
    let group_sheet = SheetRef::Shape(group_ref);
    cells.extend(["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY"].map(|name| (group_sheet.clone(), CellAddr::direct(name))));
    settle_kept(doc, page, &Changes { cells, ..Changes::default() })?;
    Ok(Grouped { id, members: shapes.len() })
}

/// The `ID` of a new shape: above every one on the page and above `above`; the page has room for one more.
pub(super) fn new_id(scan: &Scan, above: u32) -> Result<u32, Problem> {
    if scan.count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    scan.highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))
}

// ---- ungroup ---------------------------------------------------------------------------------------------------------------------

/// Takes apart the groups among the shapes named. A shape named that holds no members is left alone; when none is a group the document
/// is not touched.
pub fn ungroup_shapes(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Ungrouped, Problem> {
    let (_, shapes) = scan::named(doc, page, ids)?;
    let groups: Vec<(u32, ShapeRef)> =
        scan::outermost(shapes).into_iter().filter(|(_, at)| doc.shape(at).is_some_and(|s| !s.children.is_empty())).collect();
    if groups.is_empty() {
        return Ok(Ungrouped::default());
    }
    // A group that holds a shape on a locked layer is not taken apart: its members' cells and the formulas that name it would be rewritten.
    let named: Vec<u32> = groups.iter().map(|(id, _)| *id).collect();
    super::layers::refuse_locked_in_group(doc, page, &named, "ungrouped")?;
    if let Some((id, _)) = groups.iter().find(|(_, at)| doc.shape(at).is_some_and(|s| s.master.is_some())) {
        return Err(Problem::failed(format!(
            "shape {id} is an instance of a master, and its members take their cells from it: it cannot be ungrouped yet"
        )));
    }
    // A group inside an instance (`MasterShape` and no `Master`) is as much the master's: the names in its members' inherited formulas are bound by
    // the instance, and would find nothing once they are out of it. Copy refuses the same loss.
    if let Some((id, _)) = groups.iter().find(|(_, at)| doc.shape(at).is_some_and(|s| s.master_shape.is_some())) {
        return Err(Problem::failed(format!(
            "shape {id} is part of a group made from a master, and its members take their cells from it: it cannot be ungrouped yet"
        )));
    }
    // One id may name a group of any number of members, so the members are counted, not the ids, before any of them is handled.
    let members = groups.iter().fold(0_usize, |sum, (_, at)| sum.saturating_add(doc.shape(at).map_or(0, |g| g.children.len())));
    if members > MAX_UNGROUP_MEMBERS {
        return Err(Problem::failed(format!(
            "an ungroup takes apart at most {MAX_UNGROUP_MEMBERS} members at a time, and these groups hold {members}: ungroup fewer groups, or the inner ones first"
        )));
    }
    // Each member, with the map that carries its cells out of its group.
    let mut items: Vec<Lift> = Vec::with_capacity(members);
    {
        let reader = Reader::new(doc);
        for (_, at) in &groups {
            let by = Affine::of_pose(&reader.pose(&SheetRef::Shape(at.clone())));
            let members = doc.shape(at).map_or(0, |g| g.children.len());
            for k in 0..members {
                let k = u32::try_from(k).map_err(|_| Problem::failed("too many shapes"))?;
                let member = at.child(k);
                items.push(Lift { id: doc.shape(&member).map_or(0, |s| s.id), at: member, by });
            }
        }
    }
    let member_ids: HashSet<u32> = items.iter().map(|i| i.id).collect();
    let gone: BTreeSet<u32> = groups.iter().map(|(id, _)| *id).collect();
    // A formula of a member (or of anything inside one) that names a group that is going would be worked out against a shape that is not there, and
    // saved naming nothing: each such cell is the constant of its cached value. It comes before the lift, which leaves a connector's
    // pose cells to their formulas and so must see which still have one.
    let named: HashSet<u32> = gone.iter().copied().collect();
    freeze_names_of(doc, page, &items, &named)?;
    // A connector glued to a group itself is left with that end a plain point, and no Connect row names a group that is gone. This
    // comes first: such an end is then a constant, and is carried out with the cells of the connector, which may be a member.
    let unglued = unglue_connectors(doc, page, &gone)?;
    if let Some(p) = super::page_mut(doc, page) {
        let kept = p.connects.iter().filter(|c| !gone.contains(&c.to_sheet) && !gone.contains(&c.from_sheet)).cloned().collect();
        p.connects = Connects::from_vec(kept);
    }
    let written = lift(doc, &items, &glued_ends(doc, page, &member_ids))?;
    // Where each member is once the groups are replaced by their members: known from the rebuild, not found again by `ID`, which a page that holds
    // an `ID` twice would answer with the other shape.
    let counts: Vec<usize> = groups.iter().map(|(_, at)| doc.shape(at).map_or(0, |g| g.children.len())).collect();
    let rebuilt = replace_groups_by_members(doc, page, &groups)?;
    let paths = member_paths(&rebuilt, &groups, &counts);
    let now: Vec<(u32, ShapeRef)> = items.iter().zip(paths).map(|(item, at)| (item.id, at)).collect();
    // A group that was a member of a group leaves its members in that group: the instances among them are tied to it, as `shape.group` ties the members it
    // makes, where the ties to the group that went were made numbers above. The ties keep the numbers, so nothing is worked out again.
    let mut from = 0_usize;
    for ((_, at), count) in groups.iter().zip(&counts) {
        let upto = from.saturating_add(*count);
        let lifted = now.get(from..upto).unwrap_or_default();
        from = upto;
        let Some(parent) = lifted.first().and_then(|(_, first)| first.path.split_last()).filter(|_| at.path.len() > 1) else { continue };
        let parent = ShapeRef { owner: at.owner, path: parent.1.to_vec() };
        let Some(parent_id) = doc.shape(&parent).map(|shape| shape.id) else { continue };
        let members: Vec<ShapeRef> = lifted.iter().map(|(_, member)| member.clone()).collect();
        tie_to_group(doc, parent_id, &parent, &members, None)?;
    }
    let mut cells = reparent_changes(doc, &now, &written);
    if !unglued.is_empty() {
        // The connectors that were glued to a group are anywhere on the page, found by their `ID` as `unglue_connectors` found them.
        let wanted: HashSet<u32> = unglued.iter().copied().collect();
        let found = scan::scan(doc, page, &wanted)?;
        for id in &unglued {
            if let Some(at) = found.found.get(id) {
                let sheet = SheetRef::Shape(at.clone());
                cells.extend(["BeginX", "BeginY", "EndX", "EndY"].map(|name| (sheet.clone(), CellAddr::direct(name))));
            }
        }
    }
    let changes = Changes { cells, removed: gone.iter().copied().collect(), ..Changes::default() };
    settle_kept(doc, page, &changes)?;
    Ok(Ungrouped { groups: groups.len(), members: now.iter().map(|(id, _)| *id).collect() })
}

/// Makes each cell of the members in `items`, and of everything inside them, whose formula names a shape in `named` the constant of its cached value.
/// Iterative, and no deeper than the model allows; one pass over the members' sheets.
fn freeze_names_of(doc: &mut Document, page: u32, items: &[Lift], named: &HashSet<u32>) -> Result<(), Problem> {
    let mut stack: Vec<ShapeRef> = items.iter().map(|item| item.at.clone()).collect();
    while let Some(at) = stack.pop() {
        let Some(shape) = doc.shape(&at) else { continue };
        note_work(1);
        let (frozen, members) = (freeze_refs(&shape.sheet, named), shape.children.len());
        if !Arc::ptr_eq(&frozen, &shape.sheet) {
            shape_mut(doc, page, &at.path).ok_or_else(|| Problem::failed("a member went away"))?.sheet = frozen;
        }
        if at.path.len() < MAX_SHAPE_DEPTH {
            for k in 0..members {
                stack.push(at.child(u32::try_from(k).map_err(|_| Problem::failed("too many shapes"))?));
            }
        }
    }
    Ok(())
}

/// Where the shapes of lists that had groups replaced by their members went: by the list (as it was named, the path of the shape it is in) the places of
/// the groups in it, in order, with how many members each held.
struct Rebuilt {
    by_list: BTreeMap<Vec<u32>, ListShifts>,
}

/// The groups replaced in one list, by place, with the running total of how many places each moves the shapes after it on (its members less the one
/// place it had): a place's shift is a lookup in the total, not a sum over the groups before it.
struct ListShifts {
    places: Vec<usize>,
    /// `moved_on[i]` is the total for the groups at `places[0..=i]`.
    moved_on: Vec<usize>,
}

impl Rebuilt {
    /// The groups of each list by place, with how many members each holds.
    fn new(by_list: BTreeMap<Vec<u32>, BTreeMap<usize, usize>>) -> Rebuilt {
        let lists = by_list
            .into_iter()
            .map(|(list, groups)| {
                let (mut places, mut moved_on, mut total) = (Vec::with_capacity(groups.len()), Vec::with_capacity(groups.len()), 0_usize);
                for (place, members) in groups {
                    total = total.saturating_add(members.saturating_sub(1));
                    places.push(place);
                    moved_on.push(total);
                }
                (list, ListShifts { places, moved_on })
            })
            .collect();
        Rebuilt { by_list: lists }
    }

    /// How many places the shapes after place `index` of the list `list` moved on, for the groups before it that were replaced by their members: one
    /// search of the places and one read of the running total.
    fn shift(&self, list: &[u32], index: usize) -> usize {
        let Some(shifts) = self.by_list.get(list) else {
            note_work(1);
            return 0;
        };
        // The steps of a binary search of the places: counted, so that a test holds the work to the logarithm and not to the groups before.
        note_work(1 + usize::try_from(usize::BITS - shifts.places.len().leading_zeros()).unwrap_or(usize::MAX));
        let before = shifts.places.partition_point(|place| *place < index);
        before.checked_sub(1).and_then(|last| shifts.moved_on.get(last)).copied().unwrap_or(0)
    }

    /// The path now of the first member of the group that was at `group`: the shapes it was inside moved on for the groups before them in their lists, and
    /// it took the place of the group, after those before it. The other members are this path with their number added to the last place
    /// ([`with_place_on`]). Worked out once for a group, whatever its members.
    fn first_member_path(&self, group: &[u32]) -> Vec<u32> {
        let mut path: Vec<u32> = Vec::with_capacity(group.len());
        for (level, step) in group.iter().enumerate() {
            note_work(1);
            let index = usize::try_from(*step).unwrap_or(usize::MAX);
            let list = group.get(..level).unwrap_or_default();
            path.push(u32::try_from(index.saturating_add(self.shift(list, index))).unwrap_or(u32::MAX));
        }
        path
    }
}

/// Where the members of each group went, in the order of the groups and of the members: `counts[i]` members for `groups[i]`. The path of a group's first
/// member is worked out once for the group (a lookup for each level it is at, however many members it holds); the others are that path with the last
/// place on by one for each.
fn member_paths(rebuilt: &Rebuilt, groups: &[(u32, ShapeRef)], counts: &[usize]) -> Vec<ShapeRef> {
    let mut out: Vec<ShapeRef> = Vec::with_capacity(counts.iter().fold(0_usize, |sum, n| sum.saturating_add(*n)));
    for ((_, at), count) in groups.iter().zip(counts) {
        let first = rebuilt.first_member_path(&at.path);
        for k in 0..*count {
            note_work(1);
            out.push(ShapeRef { owner: at.owner, path: with_place_on(&first, k) });
        }
    }
    out
}

/// `path` with its last place `k` on.
fn with_place_on(path: &[u32], k: usize) -> Vec<u32> {
    let mut path = path.to_vec();
    if let Some(last) = path.last_mut() {
        *last = u32::try_from(usize::try_from(*last).unwrap_or(usize::MAX).saturating_add(k)).unwrap_or(u32::MAX);
    }
    path
}

/// In each list that holds a group named, the group is replaced by its members, in its place and in their order. Each list is rebuilt once, the
/// innermost and the latest first: a list inside a shape is found by the shape's place, which an earlier list's change would move.
fn replace_groups_by_members(doc: &mut Document, page: u32, groups: &[(u32, ShapeRef)]) -> Result<Rebuilt, Problem> {
    // The groups of each list by place, with how many members each holds.
    let mut by_list: BTreeMap<Vec<u32>, BTreeMap<usize, usize>> = BTreeMap::new();
    for (_, at) in groups {
        if let Some((last, up)) = at.path.split_last() {
            let members = doc.shape(at).map_or(0, |g| g.children.len());
            by_list.entry(up.to_vec()).or_default().insert(usize::try_from(*last).unwrap_or(usize::MAX), members);
        }
    }
    for (parent, places) in by_list.iter().rev() {
        let list = siblings(doc, page, parent).ok_or_else(|| Problem::failed("the list of shapes went away"))?;
        let mut rebuilt: Vec<Arc<Shape>> = Vec::with_capacity(list.len());
        for (i, shape) in list.iter().enumerate() {
            if places.contains_key(&i) {
                rebuilt.extend(shape.children.iter().cloned());
            } else {
                rebuilt.push(Arc::clone(shape));
            }
        }
        if rebuilt.len() > MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        put_siblings(doc, page, parent, ShapeList::from_vec(rebuilt))?;
    }
    Ok(Rebuilt::new(by_list))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// A page holding one group, `ID` 1, with `members` boxes in it, and a pin that is not at its local pin: every member is written when it is
    /// taken apart.
    fn document_with_a_group_of(members: u32) -> Document {
        let number = |text: &str| Cell::constant(diacraft_doc::Value::from_text(text));
        let boxed = |id: u32| {
            let mut shape = Shape::new(id);
            let mut sheet = Sheet::default();
            for (name, value) in [("PinX", "1"), ("PinY", "1"), ("Width", "1"), ("Height", "1"), ("LocPinX", "0.5"), ("LocPinY", "0.5")] {
                sheet.cells.insert(CellName::new(name), number(value));
            }
            shape.sheet = Arc::new(sheet);
            Arc::new(shape)
        };
        let mut group = Shape::new(1);
        let mut sheet = Sheet::default();
        for (name, value) in [("PinX", "50"), ("PinY", "50"), ("Width", "100"), ("Height", "100"), ("LocPinX", "0"), ("LocPinY", "0")] {
            sheet.cells.insert(CellName::new(name), number(value));
        }
        group.sheet = Arc::new(sheet);
        group.kind = ShapeType::Group;
        group.children = ShapeList::from_vec((0..members).map(|i| boxed(i + 2)).collect());
        let mut doc = Document::new_blank();
        if let Some(page) = super::super::page_mut(&mut doc, 1) {
            page.shapes = ShapeList::from_vec(vec![Arc::new(group)]);
        }
        doc
    }

    /// The count of work an ungroup of a group of `members` does in the steps that pair a member with the cells written under it.
    fn work_of_ungrouping(members: u32) -> usize {
        let mut doc = document_with_a_group_of(members);
        WORK.with(|w| w.set(0));
        let taken = ungroup_shapes(&mut doc, 1, &[1]).unwrap();
        assert_eq!(taken.members.len(), usize::try_from(members).unwrap());
        WORK.with(|w| w.get())
    }

    #[test]
    fn the_work_of_an_ungroup_grows_with_the_members_and_not_with_their_square() {
        // Each member is handled a fixed number of times (found, paired with its cells, named as changed): four times the members is about four
        // times the work. A search of the written cells for each member would make it sixteen.
        let (small, large) = (work_of_ungrouping(1_000), work_of_ungrouping(4_000));
        assert!(small >= 2_000, "the counter counts: {small}");
        let ratio = large as f64 / small as f64;
        assert!((3.5..4.5).contains(&ratio), "{small} for 1,000 members and {large} for 4,000: a ratio of {ratio}");
    }

    #[test]
    fn the_cells_to_work_out_after_a_hundred_thousand_members_are_listed_in_one_pass() {
        // Three cells written under each of 100,000 members: a search of the written cells for each member is 3e10 steps, which takes minutes; the
        // pass takes a fraction of a second, and counts one step for each member and each cell.
        let members = 100_000_u32;
        let doc = document_with_a_group_of(members);
        let shapes: Vec<(u32, ShapeRef)> = (0..members).map(|k| (k + 2, ShapeRef { owner: Owner::Page(1), path: vec![0, k] })).collect();
        let written: Vec<Vec<&'static str>> = (0..members).map(|_| vec!["PinX", "PinY", "Angle"]).collect();
        WORK.with(|w| w.set(0));
        let started = std::time::Instant::now();
        let cells = reparent_changes(&doc, &shapes, &written);
        let took = started.elapsed();
        assert_eq!(WORK.with(|w| w.get()), 4 * usize::try_from(members).unwrap());
        assert_eq!(cells.len(), (POSE_CELLS.len() + 3) * usize::try_from(members).unwrap());
        // A hang guard and no measure of cost: the count above is the claim (a step for each member and each cell).
        assert!(took < std::time::Duration::from_secs(60), "{took:?}");
        eprintln!("listing the cells of 100,000 members: {took:?}");
    }

    #[test]
    fn a_member_is_where_the_rebuild_put_it_whatever_was_ungrouped_before_it_in_its_list_and_in_the_lists_it_is_inside() {
        // Page list: groups at 1 (three members) and 4 (two); the shape at 5 holds a list whose group at 0 has two members.
        let rebuilt = Rebuilt::new(
            [
                (vec![], [(1, 3), (4, 2)].into_iter().collect::<BTreeMap<usize, usize>>()),
                (vec![5], [(0, 2)].into_iter().collect::<BTreeMap<usize, usize>>()),
            ]
            .into_iter()
            .collect(),
        );
        let member_path = |group: &[u32], k: usize| with_place_on(&rebuilt.first_member_path(group), k);
        // The first group's members fill its place: 1, 2, 3.
        assert_eq!((0..3).map(|k| member_path(&[1], k)).collect::<Vec<_>>(), [[1], [2], [3]]);
        // The second group was at 4: two places on for the first group's two extra members, then its own members.
        assert_eq!((0..2).map(|k| member_path(&[4], k)).collect::<Vec<_>>(), [[6], [7]]);
        // The shape at 5 is at 8 now (two before it moved it on, and one more for the second group's extra member): its group at 0 had two members.
        assert_eq!((0..2).map(|k| member_path(&[5, 0], k)).collect::<Vec<_>>(), [[8, 0], [8, 1]]);
        // A shape no group was before is only moved on.
        assert_eq!(rebuilt.shift(&[], 0), 0);
        assert_eq!(rebuilt.shift(&[], 3), 2);
        assert_eq!(rebuilt.shift(&[], 6), 3);
        assert_eq!(rebuilt.shift(&[9], 4), 0, "a list with no group in it");
    }

    #[test]
    fn the_new_paths_of_the_members_cost_a_step_for_each_member_and_a_few_for_each_group_and_not_a_sum_over_the_groups_before_it() {
        // 4,096 groups in one list, one member each but the last, which holds 46,000: a path worked out again for each member from a sum over the
        // groups before it is about 1.9e8 steps; counted here, it is a step for each member and a few for each group.
        let small = 4_096_usize;
        let big = 46_000_usize;
        let by_list: BTreeMap<Vec<u32>, BTreeMap<usize, usize>> =
            [(vec![], (0..small).map(|place| (place, 1)).chain([(small, big)]).collect::<BTreeMap<usize, usize>>())].into_iter().collect();
        let rebuilt = Rebuilt::new(by_list);
        let groups: Vec<(u32, ShapeRef)> = (0..=small)
            .map(|place| (u32::try_from(place + 1).unwrap(), ShapeRef { owner: Owner::Page(1), path: vec![u32::try_from(place).unwrap()] }))
            .collect();
        let counts: Vec<usize> = (0..small).map(|_| 1).chain([big]).collect();
        WORK.with(|w| w.set(0));
        let started = std::time::Instant::now();
        let paths = member_paths(&rebuilt, &groups, &counts);
        let took = started.elapsed();
        let work = WORK.with(|w| w.get());
        eprintln!("the paths of {} members of {} groups: {work} steps, {took:?}", paths.len(), groups.len());
        assert_eq!(paths.len(), small + big);
        // Where the groups before it put it: the last group's first member is where the group was, the groups before having one member each.
        assert_eq!(paths.get(small).map(|at| at.path.clone()), Some(vec![u32::try_from(small).unwrap()]));
        assert_eq!(paths.last().map(|at| at.path.clone()), Some(vec![u32::try_from(small + big - 1).unwrap()]));
        assert!(work < 10 * (small + big), "{work} steps for {} members and {} groups", small + big, small + 1);
        // A hang guard and no measure of cost: the count above is the claim.
        assert!(took < std::time::Duration::from_secs(60), "{took:?}");
    }

    #[test]
    fn the_new_paths_of_the_members_of_one_group_a_hundred_levels_deep_cost_in_proportion_to_the_members() {
        // One group 97 levels down holding 50,000 members: the group's path is worked out once, for its levels, and not again for each member.
        let depth = 97_usize;
        let group: Vec<u32> = (0..depth).map(|_| 0).collect();
        let by_list: BTreeMap<Vec<u32>, BTreeMap<usize, usize>> =
            [(group[..depth - 1].to_vec(), [(0, 50_000)].into_iter().collect())].into_iter().collect();
        let rebuilt = Rebuilt::new(by_list);
        let groups = vec![(1_u32, ShapeRef { owner: Owner::Page(1), path: group })];
        WORK.with(|w| w.set(0));
        let paths = member_paths(&rebuilt, &groups, &[50_000]);
        let work = WORK.with(|w| w.get());
        assert_eq!(paths.len(), 50_000);
        assert_eq!(paths.last().map(|at| at.path.len()), Some(depth));
        assert_eq!(paths.last().and_then(|at| at.path.last().copied()), Some(49_999));
        assert!(work <= 50_000 + 8 * depth, "{work} steps for 50,000 members at {depth} levels");
    }

    #[test]
    fn an_ungroup_past_the_cap_on_members_is_refused_in_words_before_any_is_handled() {
        let mut doc = document_with_a_group_of(u32::try_from(MAX_UNGROUP_MEMBERS).unwrap() + 1);
        let before = doc.clone();
        WORK.with(|w| w.set(0));
        let problem = ungroup_shapes(&mut doc, 1, &[1]).unwrap_err();
        assert!(!problem.bad_params && problem.message.contains("at most 50000 members") && problem.message.contains("50001"), "{problem:?}");
        assert_eq!(doc, before);
        assert_eq!(WORK.with(|w| w.get()), 0, "nothing was handled");
    }
}
