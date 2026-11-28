//! Dropping a master of an opened stencil on a page: the master is brought into the document (`bring.rs`), and an instance of it is
//! put where the person points. **A built-in group master is dropped by the same code** ([`drop_group_master`]): `ensure_master` has copied it in, and the instance and its members are made from it as below.
//!
//! **Everything is read from the drawing's master**: once the master is brought in, the instance is made from the master of the
//! drawing it names, which is the stencil's copy or, when finds the drawing's own, a master that may differ from the stencil's (another
//! pin, other members, a box where the stencil has a line). Its pose, whether it is a line, its `Type` and its members all come from that one,
//! so that the instance and the master it inherits from agree.
//!
//! **The instance holds what differs**: `PinX` and `PinY`, and for a one-dimensional master its four end cells. The
//! rest is the master's. Its `Type` is the master shape's, since a file's instance has its master's.
//!
//! **Members** (ours): a file's instance of a grouped master lists every descendant of the master's shape, each standing
//! for its own by `MasterShape` (all 21,355 instances of a grouped master in the sample files do), and a reader that is not
//! ours may draw nothing for a member that is not listed. So the instance lists them: one shape for each descendant, in the master's order,
//! with an `ID` of its own on the page, its master's `Type` and `MasterShape`, and no cell. **A master of more than [`MAX_LISTED_MEMBERS`]
//! descendants is dropped without its members listed**, which MS-VSDX allows and the resolver and the renderer of this project take from
//! the master (`Resolver::effective_members`): a drop of a master of tens of thousands of shapes would otherwise put as many on the page.
//!
//! **A master of several top-level shapes** (ours) **is dropped as a group** (its instance is a `Group` with a member for each top-level shape, standing for it by `MasterShape`),
//! and
//! only its frame is left open by the specifications. The frame is the **joint box of the members on the master's page** (each member's box carried
//! through its own pin, turn and flips): the group's `Width` and `Height` are the box's, its `LocPinX` and `LocPinY` the middle of it, and its `PinX`
//! and `PinY` the place (the box's middle goes there). A member's pin in the group's frame is then its pin on the master's page less the box's
//! lower-left corner, which the member holds as an override (`PinX` and `PinY`, or the four end cells of a line). A master of none is refused, and so
//! is one of more than [`MAX_LISTED_MEMBERS`] top-level shapes (each needs a member of its own on the page to stand for it).
//!
//! **A page of another drawing scale** (`draw/drop_scale.rs`): when the master's drawing scale and the page's differ by more than a factor of eight, the
//! instance keeps the master's size on paper: `Width`, `Height` and `DropOnPageScale` are written on it as the master's times the ratio, a line's ends are the master's length times
//! the ratio apart, and a master of several top-level shapes has its members that stand for them written alike, their offsets from the group's local pin times the ratio. A
//! master within the range, and a drop that follows no scale ([`Follow::Never`], Change Shape's trial instance), are as they were.

use std::sync::Arc;

use diacraft_doc::{
    Cell, CellAddr, CellName, Document, FormulaSlot, MAX_SHAPE_DEPTH, MAX_SHAPES_PER_PAGE, Owner, Resolver, Shape, ShapeList, ShapeRef, ShapeType,
    SheetRef, TextPart, Value,
};

use super::bring::{Brought, bring_master};
use super::opened::{MasterEntry, OpenedStencil};
use super::{MasterSpec, ensure_master, number_text};
use crate::draw::drop_scale::{self, Scaling, exact_length, factor_cell, size_cells};
use crate::draw::worked::Worked;
use crate::draw::{
    DropInfo, Follow, MAX_COORD, Pose, Problem, below, charge_all, checked, localise_geometry, page_mut, page_stats, require_page, shape_mut,
    tie_to_group,
};
use crate::text::shown;

/// The most descendants of a group master's shape an instance lists as members.
pub const MAX_LISTED_MEMBERS: usize = 5_000;

/// What a drop of an opened stencil's master made.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedDrop {
    pub info: DropInfo,
    pub brought: Brought,
    /// The highest `ID` the drop gave on the page: the instance's, or its last member's. The session's mark ([`crate::Session::note_id`]).
    pub highest_id: u32,
}

/// How many shapes are below `shape`, counted no further than `cap + 1`: iterative, and no deeper than the model allows.
fn descendants(shape: &Shape, cap: usize) -> usize {
    let mut count = 0_usize;
    let mut work: Vec<(&ShapeList, usize)> = vec![(&shape.children, 1)];
    while let Some((list, depth)) = work.pop() {
        count = count.saturating_add(list.iter().filter(|c| !c.del).count());
        if count > cap {
            return count;
        }
        if depth < MAX_SHAPE_DEPTH {
            work.extend(list.iter().filter(|c| !c.del && !c.children.is_empty()).map(|c| (&c.children, depth + 1)));
        }
    }
    count
}

/// The members of an instance of the group `list` belongs to: a shape for each of `list` that is not deleted, standing for it, and its own members,
/// with `ID`s taken from `next` on. Depth-first in the master's order. Recursion is as deep as the model allows and no
/// deeper. `fields` says whether a member whose master shape's text holds fields keeps that text (**a built-in group master's**: the shown form of a field lives in the
/// text that holds it, as a dropped shape's does, so the member must hold the text for the field to be refreshed when the group's Shape Data changes).
fn members(list: &ShapeList, next: &mut u32, depth: usize, fields: bool) -> Result<ShapeList, Problem> {
    let mut out: Vec<Arc<Shape>> = Vec::with_capacity(list.len());
    for child in list.iter().filter(|c| !c.del) {
        let id = *next;
        *next = next.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        let mut member = Shape::new(id);
        member.kind = child.kind.clone();
        member.master_shape = Some(child.id);
        if fields && child.text.as_ref().is_some_and(|t| t.parts.iter().any(|p| matches!(p, TextPart::Fld { .. }))) {
            member.text = child.text.clone();
        }
        if depth < MAX_SHAPE_DEPTH && !child.children.is_empty() {
            member.children = members(&child.children, next, depth + 1, fields)?;
        }
        out.push(Arc::new(member));
    }
    Ok(ShapeList::from_vec(out))
}

/// The number a cell of a master's shape holds, found by inheritance.
///
/// A cell that holds a formula and no saved value is what the formula works out to, as the picture reads it: a master sized by `=1+1` is two inches wide.
fn number(worked: &Worked<'_>, sheet: &SheetRef, name: &str) -> Option<f64> {
    worked.number_named(sheet, name).filter(|n| n.is_finite())
}

/// Whether the shape is one-dimensional: all four end cells are there after inheritance.
fn is_one_d(res: &Resolver<'_>, sheet: &SheetRef) -> bool {
    ["BeginX", "BeginY", "EndX", "EndY"]
        .iter()
        .all(|name| res.resolve(sheet, &CellAddr::direct(name)).is_some_and(|f| f.formula.cell.is_some() || f.value.cell.is_some()))
}

fn length(n: f64) -> Cell {
    Cell::constant(Value::from_text(&number_text(n))).with_unit("IN")
}

/// A name for an instance from its master's: no control characters, and no more than a name may be.
fn instance_name(master: &str, id: u32) -> Arc<str> {
    let clean: String = master.chars().filter(|c| !c.is_control()).take(200).collect();
    let clean = if clean.trim().is_empty() { "Shape".to_owned() } else { clean };
    Arc::from(format!("{clean}.{id}"))
}

/// A shape's pose and size as its master holds them.
struct Geometry {
    width: f64,
    height: f64,
    pose: Pose,
}

fn geometry(worked: &Worked<'_>, sheet: &SheetRef) -> Geometry {
    let n = |name: &str| number(worked, sheet, name);
    let (width, height) = (n("Width").unwrap_or(0.0), n("Height").unwrap_or(0.0));
    Geometry {
        width,
        height,
        pose: Pose {
            pin: (n("PinX").unwrap_or(0.0), n("PinY").unwrap_or(0.0)),
            loc: (n("LocPinX").unwrap_or(0.0), n("LocPinY").unwrap_or(0.0)),
            angle: n("Angle").unwrap_or(0.0),
            flip_x: n("FlipX").unwrap_or(0.0) != 0.0,
            flip_y: n("FlipY").unwrap_or(0.0) != 0.0,
        },
    }
}

/// The cells an instance holds, and what it is made of, worked out from the drawing's master and nothing else.
struct Layout {
    kind: ShapeType,
    /// The cells the instance holds.
    cells: Vec<(&'static str, f64)>,
    /// The cells a drop beyond the range of eight scaled, written with all their digits (the size of the shape, or the frame of a group).
    exact: Vec<(&'static str, f64)>,
    /// The ratio written as `DropOnPageScale` on the instance: a drop beyond the range of eight.
    factor: Option<f64>,
    /// Words for a ratio of scales that was left unused (the master is dropped as within the range).
    note: Option<String>,
    /// The ends of the instance when it is a line (`BeginX`, `BeginY`, `EndX`, `EndY`), so that its pin can be checked against them once what reads
    /// them is worked out ([`fix_pins`]).
    ends: Option<[f64; 4]>,
    /// The members of a group made of several top-level shapes, each with the cells it holds; empty for a master of one shape.
    members: Vec<Member>,
    /// How many shapes the instance and everything it lists are, for the page's cap.
    shapes: usize,
}

/// A member of the instance of a master of several shapes, and the cells it holds. `below` says whether the master shape's own descendants are
/// listed under it.
struct Member {
    /// The position of the master shape it stands for, among the master's top-level shapes.
    of: usize,
    cells: Vec<(&'static str, f64)>,
    /// What a drop beyond the range of eight scaled: the pin or the ends in the group's frame, and the size, written with all their digits.
    exact: Vec<(&'static str, f64)>,
    /// The ratio the member that stands for a top-level shape holds as its own `DropOnPageScale`.
    factor: Option<f64>,
    /// The ends of the member in the group's frame, for a member that is a line, checked as the instance's are.
    ends: Option<[f64; 4]>,
    below: bool,
}

/// `PinX` and `PinY` that put the centre of the box at (`x`, `y`): the centre carried through the shape's turn and flips about its pin says where
/// the pin lands relative to it.
fn centre_cells(g: &Geometry, x: f64, y: f64) -> Vec<(&'static str, f64)> {
    let at_origin = Pose { pin: (0.0, 0.0), ..g.pose };
    let offset = at_origin.up((g.width / 2.0, g.height / 2.0));
    vec![("PinX", x - offset.0), ("PinY", y - offset.1)]
}

/// The corners of a shape's box on the master's page: its own box carried through its pose.
fn page_box(g: &Geometry) -> [(f64, f64); 4] {
    [(0.0, 0.0), (g.width, 0.0), (g.width, g.height), (0.0, g.height)].map(|p| g.pose.up(p))
}

fn top_sheet(mid: u32, at: usize) -> SheetRef {
    SheetRef::Shape(ShapeRef::top(Owner::Master(mid), u32::try_from(at).unwrap_or(u32::MAX)))
}

/// What an instance of master `mid` of `doc` is, where (`x`, `y`) is the centre, or why there can be none. `scaling` is what the page's drawing scale makes of the drop
/// (`draw/drop_scale.rs`): beyond the range of eight the sizes, the ends and the offsets of a group's members are the master's times the ratio. A size, end or offset that would
/// not be a usable number makes it a drop within the range, with words.
fn layout(doc: &Document, mid: u32, x: f64, y: f64, named: &str, scaling: &Scaling) -> Result<Layout, Problem> {
    if let Some(plan) = layout_scaled(doc, mid, x, y, named, scaling)? {
        return Ok(plan);
    }
    let down = scaling.too_big();
    let mut plan = layout_scaled(doc, mid, x, y, named, &down)?.ok_or_else(|| Problem::failed("the master went away"))?;
    plan.note = down.note().map(str::to_owned);
    Ok(plan)
}

/// [`layout`] for one `scaling`: `None` when a scaled value would not be a finite number within [`MAX_COORD`].
fn layout_scaled(doc: &Document, mid: u32, x: f64, y: f64, named: &str, scaling: &Scaling) -> Result<Option<Layout>, Problem> {
    let master = doc.master(mid).ok_or_else(|| Problem::failed("the master went away"))?;
    let live: Vec<usize> = master.shapes.iter().enumerate().filter(|(_, s)| !s.del).map(|(at, _)| at).collect();
    let res = Resolver::new(doc);
    let worked = Worked::new(doc);
    let note = scaling.note().map(str::to_owned);
    match live.as_slice() {
        [] => Err(Problem::failed(format!("master `{named}` holds no shapes, so there is nothing to drop"))),
        [only] => {
            let shape = master.shapes.get(*only).ok_or_else(|| Problem::failed("the master went away"))?;
            let sheet = top_sheet(mid, *only);
            let g = geometry(&worked, &sheet);
            let ends = |name: &str| number(&worked, &sheet, name);
            let (cells, line) = match (is_one_d(&res, &sheet), (ends("BeginX"), ends("BeginY")), (ends("EndX"), ends("EndY"))) {
                (true, (Some(bx), Some(by)), (Some(ex), Some(ey))) => {
                    let moved = if scaling.beyond() {
                        // The master's length times the ratio, along its direction, centred on (x, y) (DROPSCALE-1D-1).
                        let (Some(hx), Some(hy)) = (scaling.scale((ex - bx) / 2.0), scaling.scale((ey - by) / 2.0)) else { return Ok(None) };
                        let moved = [x - hx, y - hy, x + hx, y + hy];
                        if moved.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
                            return Ok(None);
                        }
                        moved
                    } else {
                        // A line goes where the centre is: its ends are the master's, moved so that their middle is (x, y).
                        let (dx, dy) = (x - (bx + ex) / 2.0, y - (by + ey) / 2.0);
                        [bx + dx, by + dy, ex + dx, ey + dy]
                    };
                    (vec![("BeginX", moved[0]), ("BeginY", moved[1]), ("EndX", moved[2]), ("EndY", moved[3])], Some(moved))
                }
                _ => (centre_cells(&g, x, y), None),
            };
            let Some(exact) = size_cells(&worked, &sheet, scaling, line.is_some()) else { return Ok(None) };
            let below = descendants(shape, MAX_LISTED_MEMBERS);
            Ok(Some(Layout {
                kind: shape.kind.clone(),
                cells,
                exact,
                factor: scaling.factor(),
                note,
                ends: line,
                members: Vec::new(),
                shapes: 1 + if below <= MAX_LISTED_MEMBERS { below } else { 0 },
            }))
        }
        many => {
            if many.len() > MAX_LISTED_MEMBERS {
                return Err(Problem::failed(format!(
                    "master `{named}` holds {} shapes at its top level, more than the {MAX_LISTED_MEMBERS} a drop lists as the members of its group",
                    many.len()
                )));
            }
            // The joint box of the members on the master's page.
            let mut placed = Vec::with_capacity(many.len());
            let (mut left, mut bottom, mut right, mut top) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            let mut below_all = 0_usize;
            for at in many.iter().copied() {
                let sheet = top_sheet(mid, at);
                let g = geometry(&worked, &sheet);
                for (cx, cy) in page_box(&g) {
                    left = left.min(cx);
                    bottom = bottom.min(cy);
                    right = right.max(cx);
                    top = top.max(cy);
                }
                if let Some(shape) = master.shapes.get(at) {
                    below_all = below_all.saturating_add(descendants(shape, MAX_LISTED_MEMBERS));
                }
                placed.push((at, is_one_d(&res, &sheet), g));
            }
            let (width, height) = ((right - left).max(0.0), (top - bottom).max(0.0));
            let below = many.len().saturating_add(below_all) <= MAX_LISTED_MEMBERS;
            let (cells, exact) = if scaling.beyond() {
                // The frame is the box times the ratio, the local pin its middle.
                let (Some(w), Some(h)) = (scaling.scale(width), scaling.scale(height)) else { return Ok(None) };
                (vec![("PinX", x), ("PinY", y)], vec![("Width", w), ("Height", h), ("LocPinX", w / 2.0), ("LocPinY", h / 2.0)])
            } else {
                (
                    vec![("Width", width), ("Height", height), ("LocPinX", width / 2.0), ("LocPinY", height / 2.0), ("PinX", x), ("PinY", y)],
                    Vec::new(),
                )
            };
            let mut members: Vec<Member> = Vec::with_capacity(placed.len());
            for (at, one_d, g) in placed {
                // The member's cells are its master's pin (or the ends of a line) in the frame of the group: the box's lower-left corner is
                // the origin.
                let sheet = top_sheet(mid, at);
                let (mut cells, mut exact, mut ends) = (Vec::new(), Vec::new(), None);
                if one_d {
                    let n = |name: &str| number(&worked, &sheet, name).unwrap_or(0.0);
                    let raw = [n("BeginX") - left, n("BeginY") - bottom, n("EndX") - left, n("EndY") - bottom];
                    let names = ["BeginX", "BeginY", "EndX", "EndY"];
                    let mut scaled = raw;
                    if scaling.beyond() {
                        for (slot, v) in scaled.iter_mut().zip(raw) {
                            let Some(v) = scaling.scale(v) else { return Ok(None) };
                            *slot = v;
                        }
                        exact = names.into_iter().zip(scaled).collect();
                    } else {
                        cells = names.into_iter().zip(scaled).collect();
                    }
                    ends = Some(scaled);
                } else {
                    let pin = (g.pose.pin.0 - left, g.pose.pin.1 - bottom);
                    if scaling.beyond() {
                        let (Some(px), Some(py)) = (scaling.scale(pin.0), scaling.scale(pin.1)) else { return Ok(None) };
                        exact = vec![("PinX", px), ("PinY", py)];
                    } else {
                        cells = vec![("PinX", pin.0), ("PinY", pin.1)];
                    }
                }
                // The member that stands for a top-level shape is sized as the top-level shape is.
                let Some(sizes) = size_cells(&worked, &sheet, scaling, one_d) else { return Ok(None) };
                exact.extend(sizes);
                members.push(Member { of: at, cells, exact, factor: scaling.factor(), ends, below });
            }
            Ok(Some(Layout {
                kind: ShapeType::Group,
                cells,
                exact,
                factor: scaling.factor(),
                note,
                ends: None,
                shapes: 1 + many.len() + if below { below_all } else { 0 },
                members,
            }))
        }
    }
}

/// How far along a line, from its begin to its end, its pin is: `LocPinX` of its `Width` (the pin is midway only
/// when `LocPinX` is half the width). A line whose `Width` or `LocPinX` cannot be read, or whose width is nothing, has its pin midway.
fn along(worked: &Worked<'_>, sheet: &SheetRef) -> f64 {
    match (number(worked, sheet, "LocPinX"), number(worked, sheet, "Width")) {
        (Some(loc), Some(width)) if width > 1e-9 => loc / width,
        _ => 0.5,
    }
}

/// A line's pin, set where its ends and its `LocPinX` put it, when it is a number and not where they put it: the cells set, with the path below the
/// instance of the shape each is on, for what reads them to be worked out again.
///
/// A master holds the pin of a line as numbers (a file may, and a pin a formula of the ends is worked out already by the instance's ends). The
/// number is a place on the master's page, which a drop has moved the ends away from, so the instance gets a pin of its own: `begin + (LocPinX /
/// Width) * (end - begin)`, the point of the line `LocPinX` names, which is the middle only when `LocPinX` is half the width. **A
/// pin that has a formula is left as it is**: the recalculation that follows the ends has worked it out, and it is the master's statement of where
/// the pin is. A member's pin is in its group's frame, and the master's number is in the master's page.
fn fix_pins(doc: &mut Document, page: u32, at: &ShapeRef, lines: &[(Vec<u32>, [f64; 4])]) -> Result<Vec<(Vec<u32>, &'static str)>, Problem> {
    let now = Resolver::new(doc);
    let worked = Worked::new(doc);
    let mut fixes: Vec<(Vec<u32>, &'static str, f64)> = Vec::new();
    for (below, ends) in lines {
        let mut path = at.clone();
        path.path.extend(below.iter().copied());
        let sheet = SheetRef::Shape(path);
        let t = along(&worked, &sheet);
        for (name, want) in [("PinX", ends[0] + t * (ends[2] - ends[0])), ("PinY", ends[1] + t * (ends[3] - ends[1]))] {
            let Some(found) = now.resolve(&sheet, &CellAddr::direct(name)) else { continue };
            let formula = found.formula.cell.is_some_and(|c| matches!(c.formula, FormulaSlot::Local(_)));
            let off = number(&worked, &sheet, name).is_some_and(|got| (got - want).abs() > 1e-9);
            if off && !formula {
                fixes.push((below.clone(), name, want));
            }
        }
    }
    drop(now);
    drop(worked);
    let mut set = Vec::with_capacity(fixes.len());
    for (below, name, want) in fixes {
        let mut path = at.path.clone();
        path.extend(below.iter().copied());
        if let Some(shape) = shape_mut(doc, page, &path) {
            shape.counted_sheet().set_cell(&CellAddr::direct(name), length(checked(name, want)?)).map_err(|e| Problem::failed(e.to_string()))?;
            set.push((below, name));
        }
    }
    Ok(set)
}

/// Drops master `entry` of `stencil` centred at (`x`, `y`) on `page`, in the page's drawing units, with an `ID` above `above` and above every
/// `ID` on the page ([`crate::draw::drop_shape_above`] says why). The document is as it was when this fails.
pub fn drop_opened(
    doc: &mut Document,
    page: u32,
    stencil: &OpenedStencil,
    entry: &MasterEntry,
    x: f64,
    y: f64,
    above: u32,
) -> Result<OpenedDrop, Problem> {
    drop_opened_following(doc, page, stencil, entry, (x, y), above, Follow::Page)
}

/// [`drop_opened`] that says whether the drop follows the page's drawing scale ([`Follow::Page`]: every drop; [`Follow::Never`]: Change Shape's trial instance, which is not a drop on a page).
pub fn drop_opened_following(
    doc: &mut Document,
    page: u32,
    stencil: &OpenedStencil,
    entry: &MasterEntry,
    (x, y): (f64, f64),
    above: u32,
    follow: Follow,
) -> Result<OpenedDrop, Problem> {
    // The master is brought in before the instance is made from it, and a refusal after that must not leave it there.
    let saved = doc.clone();
    let done = drop_into(doc, page, stencil, entry, (x, y), above, follow);
    if done.is_err() {
        *doc = saved;
    }
    done
}

fn drop_into(
    doc: &mut Document,
    page: u32,
    stencil: &OpenedStencil,
    entry: &MasterEntry,
    (x, y): (f64, f64),
    above: u32,
    follow: Follow,
) -> Result<OpenedDrop, Problem> {
    let (x, y) = (checked("x", x)?, checked("y", y)?);
    require_page(doc, page)?;
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let named = shown(&entry.name, 64);
    if entry.top_shapes == 0 {
        return Err(Problem::failed(format!("master `{named}` holds no shapes, so there is nothing to drop")));
    }
    let brought = bring_master(doc, stencil.document(), entry.position, stencil.identity(entry))
        .map_err(|r| Problem::failed(format!("master `{named}` cannot be dropped: {r}")))?;
    let placed = Placement { page, master: brought.id, id, count, at: (x, y), named: &named, fields: false, follow };
    let (info, highest_id) = place(doc, &placed)?;
    Ok(OpenedDrop { info, brought, highest_id })
}

/// Drops the built-in group master `spec` centred at (`x`, `y`) on `page`, in the page's drawing units, with an `ID` above `above` and above every `ID` on the page
/// ([`crate::draw::drop_shape_above`] says why): the same drop as a stencil file's master of a group (are one code), **the
/// instance holds only its pin and lists a member for each member of the master**, each with a local copy of its master shape's geometry, and the text of a member whose master shape's text holds
/// fields. The document is as it was when this fails.
pub(crate) fn drop_group_master(
    doc: &mut Document,
    page: u32,
    spec: &MasterSpec,
    (x, y): (f64, f64),
    above: u32,
    follow: Follow,
) -> Result<DropInfo, Problem> {
    let saved = doc.clone();
    let done = drop_group_inner(doc, page, spec, (x, y), above, follow);
    if done.is_err() {
        *doc = saved;
    }
    done
}

fn drop_group_inner(doc: &mut Document, page: u32, spec: &MasterSpec, (x, y): (f64, f64), above: u32, follow: Follow) -> Result<DropInfo, Problem> {
    let (x, y) = (checked("x", x)?, checked("y", y)?);
    require_page(doc, page)?;
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count >= MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let id = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    let master = ensure_master(doc, spec)?;
    let placed = Placement { page, master, id, count, at: (x, y), named: &shown(spec.name, 64), fields: true, follow };
    place(doc, &placed).map(|(info, _)| info)
}

/// Where and how an instance of a master the document holds is put on a page.
struct Placement<'a> {
    page: u32,
    /// The `ID` of the master in the document.
    master: u32,
    /// The instance's `ID`, which every one it lists is above.
    id: u32,
    /// How many shapes the page holds before the drop.
    count: usize,
    /// Where the middle goes.
    at: (f64, f64),
    /// The master's name for a refusal's words.
    named: &'a str,
    /// Whether a member whose master shape's text holds fields keeps that text ([`members`]).
    fields: bool,
    /// Whether the drop follows the page's drawing scale (`draw/drop_scale.rs`).
    follow: Follow,
}

/// Puts an instance of the master `placed.master`, which `doc` holds, on the page, and lists its members. → what the drop made, and the highest `ID` it gave.
fn place(doc: &mut Document, placed: &Placement<'_>) -> Result<(DropInfo, u32), Problem> {
    let Placement { page, master: master_id, id, count, at: (x, y), named, fields, follow } = *placed;
    // From here on everything is read from the drawing's master, the one the instance names.
    let master = Arc::clone(doc.master(master_id).ok_or_else(|| Problem::failed("the master went away"))?);
    let scaling = if follow == Follow::Never { Scaling::none() } else { Scaling::read(doc, page, master_id) };
    let plan = layout(doc, master_id, x, y, named, &scaling)?;
    if count.saturating_add(plan.shapes) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let name = instance_name(master.name.as_deref().or(master.name_u.as_deref()).unwrap_or("Shape"), id);
    let mut instance = Shape::new(id);
    instance.name = Some(Arc::clone(&name));
    instance.name_u = Some(Arc::clone(&name));
    instance.kind = plan.kind.clone();
    instance.master = Some(master_id);
    let mut sheet = diacraft_doc::Sheet::default();
    for (cell, value) in &plan.cells {
        sheet.cells.insert(CellName::new(cell), length(checked(cell, *value)?));
    }
    for (cell, value) in &plan.exact {
        sheet.cells.insert(CellName::new(cell), exact_length(checked(cell, *value)?));
    }
    if let Some(factor) = plan.factor {
        sheet.cells.insert(CellName::new("DropOnPageScale"), factor_cell(factor));
    }
    instance.sheet = Arc::new(sheet);

    // The members, listed as a file lists them: a shape for each descendant, with an `ID` of its own on the page (and the cells that place a group's
    // members in the group's frame).
    let mut next = id.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
    // Every cell written, with the path below the instance of the shape it is on.
    let mut written: Vec<(Vec<u32>, &'static str)> = plan
        .cells
        .iter()
        .chain(&plan.exact)
        .map(|(cell, _)| (Vec::new(), *cell))
        .chain(plan.factor.map(|_| (Vec::new(), "DropOnPageScale")))
        .collect();
    // The lines among them (the instance and its members) and their ends, with the path below the instance of the shape.
    let mut lines: Vec<(Vec<u32>, [f64; 4])> = plan.ends.map(|e| (Vec::new(), e)).into_iter().collect();
    if plan.members.is_empty() {
        // One top-level shape: its members are its own descendants, when there are few enough.
        let live = master.shapes.iter().position(|s| !s.del);
        if let Some(shape) = live.and_then(|at| master.shapes.get(at))
            && plan.shapes > 1
        {
            instance.children = members(&shape.children, &mut next, 1, fields)?;
        }
    } else {
        let mut listed: Vec<Arc<Shape>> = Vec::with_capacity(plan.members.len());
        for member in &plan.members {
            let Some(of) = master.shapes.get(member.of) else { continue };
            // The one index the member has below the instance, for the cells written and for the pins checked alike: its place in `listed`.
            let slot = u32::try_from(listed.len()).unwrap_or(u32::MAX);
            if let Some(ends) = member.ends {
                lines.push((vec![slot], ends));
            }
            let member_id = next;
            next = next.checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
            let mut shape = Shape::new(member_id);
            shape.kind = of.kind.clone();
            shape.master_shape = Some(of.id);
            let mut sheet = diacraft_doc::Sheet::default();
            for (cell, value) in &member.cells {
                sheet.cells.insert(CellName::new(cell), length(checked(cell, *value)?));
                written.push((vec![slot], *cell));
            }
            for (cell, value) in &member.exact {
                sheet.cells.insert(CellName::new(cell), exact_length(checked(cell, *value)?));
                written.push((vec![slot], *cell));
            }
            if let Some(factor) = member.factor {
                sheet.cells.insert(CellName::new("DropOnPageScale"), factor_cell(factor));
                written.push((vec![slot], "DropOnPageScale"));
            }
            shape.sheet = Arc::new(sheet);
            if member.below && !of.children.is_empty() {
                shape.children = members(&of.children, &mut next, 2, fields)?;
            }
            listed.push(Arc::new(shape));
        }
        instance.children = ShapeList::from_vec(listed);
    }
    let highest_id = next.saturating_sub(1).max(id);
    let instance_children = instance.children.len();
    page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?.push_shape(Arc::new(instance));

    // What reads a cell the instance now holds is worked out again: a formula inherited from the master that reads `PinX`, or the pin and size of
    // a line that read its ends.
    let at = doc.find_shape(Owner::Page(page), id).ok_or_else(|| Problem::failed("the new shape went away"))?;
    let below_at = |below: &[u32], cell: &str| {
        let mut path = at.clone();
        path.path.extend(below.iter().copied());
        (SheetRef::Shape(path), CellAddr::direct(cell))
    };
    drop_scale::recalculate(doc, page, written.iter().map(|(below, cell)| below_at(below, cell)).collect())?;
    // Then the pins of the lines, set where a number the master holds has gone stale, and what reads them is worked out in turn.
    let fixed = fix_pins(doc, page, &at, &lines)?;
    drop_scale::recalculate(doc, page, fixed.iter().map(|(below, cell)| below_at(below, cell)).collect())?;
    // A shape of one box that was scaled has its pin put where the new box has its middle at the drop point: the local pin is the master's formula, which follows the size and which
    // the drop does not write.
    if plan.factor.is_some() && plan.members.is_empty() && plan.ends.is_none() {
        drop_scale::recentre(doc, page, &at, (x, y))?;
    }
    // The members of a master of several shapes are in the instance's frame, which the master has no shape of: their size and pin are formulas of the
    // instance, as a file's nested shapes are. The members of a master of one group shape inherit its
    // formulas and hold nothing (absent or `Inh` on 94% of the members of an instance).
    if !plan.members.is_empty() {
        let members: Vec<ShapeRef> = (0..instance_children).filter_map(|k| u32::try_from(k).ok()).map(|k| at.child(k)).collect();
        tie_to_group(doc, id, &at, &members, None)?;
    }
    // Each member listed carries the geometry of the master shape it stands for (kept by so that a reader that does not
    // know the master draws the members). The instance does not: it is on the page, and holds only what differs. A master whose members' geometry is
    // more than one command copies is dropped with them listed and none copied (MS-VSDX lets an instance hold only what differs): a drop is not
    // refused for it.
    let listed = below(doc, &at);
    if charge_all(doc, &listed).is_ok() {
        localise_geometry(doc, page, &listed)?;
    }
    let worked = Worked::new(doc);
    let sheet = SheetRef::Shape(at);
    let (width, height) = (number(&worked, &sheet, "Width").unwrap_or(0.0), number(&worked, &sheet, "Height").unwrap_or(0.0));
    Ok((DropInfo { id, master: master_id, name: name.to_string(), width, height, drop_scale: plan.factor, note: plan.note }, highest_id))
}
