//! The Cross-Functional Flowchart: a structure of a **frame** that holds a **list of lanes** (and a **list of phases**), made, added to, reordered, resized, turned and labelled.
//! The rules of the layout are in [`super::lists`]; the shapes are `stencil/swimlane.rs`'s; `docs/editing-commands.md` ("Swimlanes") has them all.
//!
//! **The model is the ShapeSheet, and nothing else** (B1): the frame holds the two lists under code 1 and each list
//! names it under 4; a list holds its lanes (or phases) under 2, in order, and each names the list under 5; a lane holds the shapes dropped in it under 1 and each names it under 4. A shape is
//! a part of a structure by the `User.DiaCraftSwimlane` row it has (ours); the formula engine's `LISTMEMBERCOUNT`, `LISTORDER` and `LISTSHEETREF` read the same cells.
//!
//! * Our rule. A structure is made with its first lane's top left corner where it is asked, lanes of one thickness (1.25 inches when none is said), all as long as asked, a
//!   phase band above the lanes (to their left when the lanes are vertical), and a placeholder label, `Function` for a lane and `Phase` for a phase, a person types over. It holds no shape
//!   that lies under it: a shape joins a lane when it is dropped or moved inside one, as for a container.
//! * (ours: the specifications say a lane is added before or after one, from the header's menu, a corner arrow, the tab's button or by dragging from the stencil, and
//!   at the end when none is selected). The new lane is as thick as the lane it is put next to (the one before it, for a new first lane), carries the label of its own kind, and the lanes
//!   after it make room, taking what they hold. A **separator** splits a phase in two (at the place it is dropped, or in the middle): the length of the lanes is not changed by it. A
//!   structure with no phases is given a list of two when its first separator is added.
//! * (ours: E1 says the tab can change the orientation and no page says what happens to existing lanes). The whole structure is turned over its diagonal, about its top left
//!   corner: the lane that was the k-th is the k-th, a row becomes a column, the phase band moves to the other side and each label's strip goes with it (left and top change places),
//!   and what a lane holds keeps its place in it (each shape's middle is carried over the same diagonal, and it is not resized).
//! * Our rule: "Rotate Lane Label" turns the label's direction. The label runs up the lane's left side (a horizontal lane) or along its top (a vertical lane); **rotating**
//!   moves its strip to the other of the two, so that the text turns a quarter, and the text a person typed stays.

use std::collections::HashSet;
use std::sync::Arc;

use diacraft_doc::{Cell, CellName, Document, MAX_SHAPES_PER_PAGE, Shape, Value};

use super::lists::{
    Anchor, Dir, MAX_LIST_MEMBERS, MIN_INCHES, Outline, PHASE_BAND_INCHES, Role, Screen, Structure, Trigger, direction, outline_of, own_role,
    relayout, role, structure_of,
};
use super::membership::Graph;
use super::page::{Page, Rect, Unique};
use super::settings::{BAND_INCHES, Settings, number_cell, put as put_row};
use crate::draw::ties::{IN_CONTAINER, IN_LIST, LIST_MEMBERS, MEMBERS, Ties};
use crate::draw::{Problem, checked, page_mut, page_stats, require_page, require_shape, units_per_inch};
use crate::stencil::container::Side;
use crate::stencil::swimlane::{DIRECTION_ROW, LANE_LABEL, PHASE_LABEL, Part, frame_shape, lane_shape, list_shape, phase_shape};

/// The most lanes, and the most phases, one command makes or a structure is given (a list is laid out up to [`MAX_LIST_MEMBERS`]).
pub const MAX_NEW: usize = 256;

/// The thickness of a lane made with none said, in paper inches.
pub const DEFAULT_THICKNESS_INCHES: f64 = 1.25;

/// The room a Fit to Contents leaves round what a lane holds, in paper inches.
pub const LANE_MARGIN_INCHES: f64 = 0.1;

/// A structure to make.
#[derive(Clone, Copy, Debug)]
pub struct Create<'a> {
    /// How the lanes lie: one under the next (`Down`, horizontal lanes) or side by side (`Across`, vertical lanes).
    pub dir: Dir,
    pub lanes: usize,
    /// How many phases (none for no phase list).
    pub phases: usize,
    /// The top left corner of the first lane, on the page, in drawing units (y up).
    pub left: f64,
    pub top: f64,
    /// How long every lane is, and how thick, in drawing units.
    pub length: f64,
    pub thickness: f64,
    pub lane_label: &'a str,
    pub phase_label: &'a str,
    /// The `ID` the session's high-water mark keeps new shapes above.
    pub above: u32,
}

/// What a structure made is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub frame: u32,
    pub lane_list: u32,
    pub lanes: Vec<u32>,
    pub phase_list: Option<u32>,
    pub phases: Vec<u32>,
}

fn least_words(what: &str) -> Problem {
    Problem::bad(format!("{what} must be at least {MIN_INCHES} inch"))
}

/// The side a lane's label is on when it has not been rotated: the left of a lane that lies across the page, the top of one that lies down it.
pub fn lane_side(dir: Dir) -> Side {
    match dir {
        Dir::Down => Side::Left,
        Dir::Across => Side::Top,
    }
}

/// The side a phase's label is on: the top of a band along the top, the left of one along the left.
pub fn phase_side(dir: Dir) -> Side {
    match dir {
        Dir::Down => Side::Top,
        Dir::Across => Side::Left,
    }
}

fn part<'a>(id: u32, b: Screen, side: Side, band: f64, margin: f64, text: &'a str) -> Part<'a> {
    let r = b.rect();
    Part { id, centre: r.centre(), size: (r.width().max(1.0e-3), r.height().max(1.0e-3)), side, band, margin, text }
}

/// Puts the ties on the shape's `Relationships` cell (nothing when it has none).
fn set_ties(shape: &mut Shape, ties: &Ties) {
    let source = ties.print();
    if source == "SUM()" {
        return;
    }
    Arc::make_mut(&mut shape.sheet).cells.insert(CellName::new("Relationships"), Cell::with_formula(&source, Some(Value::from_text("0"))));
}

/// The boxes of a list of `n` phases that lie along the lanes: a band of `band` along the first side of the lanes' box `lanes` (above them for `Dir::Down`, left of them for `Dir::Across`), the
/// phases of the widths `widths` one after the other. → each phase's box, and the box of the band.
fn band_boxes(dir: Dir, lanes: Screen, band: f64, widths: &[f64]) -> (Vec<Screen>, Option<Screen>) {
    let mut at = 0.0_f64;
    let mut all: Option<Screen> = None;
    let mut out = Vec::with_capacity(widths.len());
    for w in widths {
        let b = match dir {
            Dir::Down => Screen { l: lanes.l + at, t: lanes.t - band, r: lanes.l + at + w, b: lanes.b },
            Dir::Across => Screen { l: lanes.l - band, t: lanes.t + at, r: lanes.r, b: lanes.t + at + w },
        };
        at += w;
        all = Some(all.map_or(b, |a| a.union(b)));
        out.push(b);
    }
    (out, all)
}

/// Makes a structure on page `page` (`swimlane.create`): the frame, the list of lanes and the lanes, and, for `phases` more than none, the list of phases and the phases, all at the back of the
/// page, the frame furthest back (the lanes are in front of the phases, so that a click in a lane reaches it).
pub fn create(doc: &mut Document, page: u32, req: Create<'_>) -> Result<Created, Problem> {
    require_page(doc, page)?;
    super::super::text_edit::check_text(req.lane_label)?;
    super::super::text_edit::check_text(req.phase_label)?;
    if req.lanes == 0 || req.lanes > MAX_NEW {
        return Err(Problem::bad(format!("`lanes` must be from 1 to {MAX_NEW}")));
    }
    if req.phases > MAX_NEW {
        return Err(Problem::bad(format!("`phases` must be at most {MAX_NEW}")));
    }
    let k = units_per_inch(doc, page);
    let (left, top) = (checked("x", req.left)?, checked("y", req.top)?);
    let (length, thickness) = (checked("length", req.length)?, checked("thickness", req.thickness)?);
    let least = MIN_INCHES * k;
    if length < least {
        return Err(least_words("`length`"));
    }
    if thickness < least {
        return Err(least_words("`thickness`"));
    }
    let n_shapes = 2 + req.lanes + if req.phases > 0 { 1 + req.phases } else { 0 };
    let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
    if count.saturating_add(n_shapes) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let mut next = highest.max(req.above);
    let mut fresh = |what: &str| -> Result<u32, Problem> {
        next = next.checked_add(1).ok_or_else(|| Problem::failed(format!("no shape ids are left on the page for the {what}")))?;
        Ok(next)
    };
    let (frame_id, lane_list_id) = (fresh("frame")?, fresh("lane list")?);
    let lane_ids: Vec<u32> = (0..req.lanes).map(|_| fresh("lane")).collect::<Result<_, _>>()?;
    let (phase_list_id, phase_ids) = if req.phases > 0 {
        let list = fresh("phase list")?;
        (Some(list), (0..req.phases).map(|_| fresh("phase")).collect::<Result<Vec<u32>, _>>()?)
    } else {
        (None, Vec::new())
    };
    let (l, t) = (left, -top);
    let band = BAND_INCHES * k;
    let margin = LANE_MARGIN_INCHES * k;
    let phase_band = PHASE_BAND_INCHES * k;
    let lane_box = |i: usize| {
        let (a, b) = (thickness * i as f64, thickness * (i as f64 + 1.0));
        match req.dir {
            Dir::Down => Screen { l, t: t + a, r: l + length, b: t + b },
            Dir::Across => Screen { l: l + a, t, r: l + b, b: t + length },
        }
    };
    let lanes_box = (0..req.lanes).map(lane_box).reduce(Screen::union).ok_or_else(|| Problem::failed("a structure with no lanes"))?;
    let phase_widths = vec![length / req.phases.max(1) as f64; req.phases];
    let (phase_boxes, band_box) = band_boxes(req.dir, lanes_box, phase_band, &phase_widths);
    let whole = band_box.map_or(lanes_box, |b| lanes_box.union(b));

    let mut shapes: Vec<Shape> = Vec::with_capacity(n_shapes);
    // The frame holds the lists, and each list names it back.
    let mut frame_ties = Ties::default();
    frame_ties.add(MEMBERS, lane_list_id);
    let mut frame = frame_shape(&part(frame_id, whole, Side::Top, band, 0.0, ""));
    if let Some(list) = phase_list_id {
        frame_ties.add(MEMBERS, list);
    }
    set_ties(&mut frame, &frame_ties);
    shapes.push(frame);
    if let Some(list_id) = phase_list_id {
        let mut ties = Ties::default();
        ties.add(IN_CONTAINER, frame_id);
        for id in &phase_ids {
            ties.add(LIST_MEMBERS, *id);
        }
        let mut list = list_shape(
            &part(list_id, band_box.unwrap_or(lanes_box), Side::Top, band, 0.0, ""),
            crate::stencil::swimlane::PHASES,
            req.dir.turned().number(),
        );
        set_ties(&mut list, &ties);
        shapes.push(list);
        for (id, b) in phase_ids.iter().zip(&phase_boxes) {
            let mut ties = Ties::default();
            ties.add(IN_LIST, list_id);
            let mut phase = phase_shape(&part(*id, *b, phase_side(req.dir), band, 0.0, req.phase_label));
            set_ties(&mut phase, &ties);
            shapes.push(phase);
        }
    }
    let mut ties = Ties::default();
    ties.add(IN_CONTAINER, frame_id);
    for id in &lane_ids {
        ties.add(LIST_MEMBERS, *id);
    }
    let mut lane_list = list_shape(&part(lane_list_id, lanes_box, Side::Top, band, 0.0, ""), crate::stencil::swimlane::LANES, req.dir.number());
    set_ties(&mut lane_list, &ties);
    shapes.push(lane_list);
    for (i, id) in lane_ids.iter().enumerate() {
        let mut ties = Ties::default();
        ties.add(IN_LIST, lane_list_id);
        let mut lane = lane_shape(&part(*id, lane_box(i), lane_side(req.dir), band, margin, req.lane_label));
        set_ties(&mut lane, &ties);
        shapes.push(lane);
    }
    let list = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    for (at, shape) in shapes.into_iter().enumerate() {
        list.shapes.insert(at, Arc::new(shape)).map_err(|_| Problem::failed("the page's list of shapes is too long"))?;
    }
    Ok(Created { frame: frame_id, lane_list: lane_list_id, lanes: lane_ids, phase_list: phase_list_id, phases: phase_ids })
}

/// Where a lane or a phase is put next to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Before,
    After,
}

/// A lane or phase that was added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inserted {
    pub id: u32,
    pub list: u32,
    /// Its place in the list, from 0.
    pub index: usize,
    /// The list of phases that was made for it, when the structure had none; and the phases then made.
    pub made: Option<Created>,
}

/// The structure of the first list of lanes the page holds that DiaCraft lays out, for a command that names no shape.
fn first_structure(page: &Page<'_>) -> Result<Option<Structure>, Problem> {
    for id in &page.containers {
        if role(page, *id)? == Role::Lanes
            && direction(page, *id)?.is_some()
            && let Some(s) = structure_of(page, *id)?
        {
            return Ok(Some(s));
        }
    }
    Ok(None)
}

fn none_words() -> Problem {
    Problem::bad(
        "the page has no swimlanes: `swimlane.create` makes a structure of lanes (File > New > Cross-Functional Flowchart starts a drawing with one)",
    )
}

fn not_part(id: u32) -> Problem {
    Problem::bad(format!("shape {id} is not a part of a cross-functional flowchart (a frame, a list, a lane or a phase)"))
}

/// The structure for a command that names `target`, or the page's first when it names none.
fn structure_for(page: &Page<'_>, target: Option<u32>) -> Result<Structure, Problem> {
    match target {
        Some(id) => {
            if !page.has(id) {
                return Err(Problem::bad(format!("page {} has no shape {id}", page.id)));
            }
            structure_of(page, id)?.ok_or_else(|| not_part(id))
        }
        None => first_structure(page)?.ok_or_else(none_words),
    }
}

/// Puts the members of the list `list` in the order `order`.
pub(super) fn set_order(doc: &mut Document, page: u32, list: u32, order: &[u32]) -> Result<(), Problem> {
    let graph = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        graph.list_order(&p, list, order)?;
        graph
    };
    graph.write(doc, page)?;
    Ok(())
}

/// Writes the box `to` for the shape `id` and settles the page, so that what the next read of the page finds is the box (a read of a derived cell before the recalculation finds the old size):
/// the cells of the frame, the outline made again for the size, and one recalculation.
fn write_box(doc: &mut Document, page: u32, id: u32, to: Rect) -> Result<(), Problem> {
    let outline = {
        let p = Page::read(doc, page)?;
        outline_of(&p, id)?
    };
    let before = doc.clone();
    crate::draw::shapes::resize_cells(doc, page, id, to.x0, to.y0, to.width(), to.height())?;
    if let Some((at, style, side, band)) = outline {
        super::regeometry(doc, page, at, style, side, (to.width(), to.height()), band)?;
    }
    crate::draw::settle(doc, &before, page, &[id], &[])
}

/// The place in the page's list after the last of the shapes among `ids` (and after `floor`): where a new shape goes, at the back with the structure and in front of what it lies on.
fn after_all(p: &Page<'_>, ids: &[u32], floor: u32) -> usize {
    ids.iter().chain(std::iter::once(&floor)).filter_map(|id| p.at(*id)).max().map_or(0, |at| at + 1)
}

fn insert_at(doc: &mut Document, page: u32, at: usize, shape: Shape) -> Result<(), Problem> {
    let list = page_mut(doc, page).ok_or_else(|| Problem::failed("the page went away"))?;
    list.shapes.insert(at.min(list.shapes.len()), Arc::new(shape)).map_err(|_| Problem::failed("the page's list of shapes is too long"))
}

/// A lane added to a structure (`swimlane.insert`). `target` is a lane (the new one goes before or after it), another part of the structure (it goes at the end), or none (the page's first
/// structure, at the end).
pub fn insert_lane(doc: &mut Document, page: u32, target: Option<u32>, place: Place, label: &str, above: u32) -> Result<Inserted, Problem> {
    require_page(doc, page)?;
    super::super::text_edit::check_text(label)?;
    let (s, index, new, at, list_id) = {
        let p = Page::read(doc, page)?;
        let s = structure_for(&p, target)?;
        let dir = direction(&p, s.lanes)?.ok_or_else(|| not_part(s.lanes))?;
        let order = p.list_members_of(s.lanes)?;
        if order.len() >= MAX_NEW {
            return Err(Problem::failed(format!("a swimlane list holds at most {MAX_NEW} lanes")));
        }
        let at_lane = target.and_then(|t| order.iter().position(|l| *l == t));
        let index = match (at_lane, place) {
            (Some(i), Place::Before) => i,
            (Some(i), Place::After) => i + 1,
            (None, _) => order.len(),
        };
        let reference = order.get(index.saturating_sub(1)).or_else(|| order.first()).copied();
        let list_box = p.bounds(s.lanes)?.map(Screen::of).ok_or_else(|| Problem::failed("the list of lanes has no box"))?;
        let (thick, side) = match reference {
            Some(r) => {
                let b = p.bounds(r)?.map(Screen::of).ok_or_else(|| Problem::failed(format!("lane {r} has no box")))?;
                (b.main(dir), side_beside(&p, r, lane_side(dir))?)
            }
            None => (DEFAULT_THICKNESS_INCHES * p.k, lane_side(dir)),
        };
        let thick = thick.max(MIN_INCHES * p.k);
        let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
        if count >= MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        let new = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        // At the end of the list for now: the layout puts it in its place and the lanes after it make room.
        let b = match dir {
            Dir::Down => Screen { l: list_box.l, t: list_box.b, r: list_box.r, b: list_box.b + thick },
            Dir::Across => Screen { l: list_box.r, t: list_box.t, r: list_box.r + thick, b: list_box.b },
        };
        let shape = lane_shape(&part(new, b, side, BAND_INCHES * p.k, LANE_MARGIN_INCHES * p.k, label));
        let at = after_all(&p, &order, s.lanes);
        (s, index, new, (at, shape), s.lanes)
    };
    let (place_at, shape) = at;
    insert_at(doc, page, place_at, shape)?;
    let graph = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        graph.list_put(&p, list_id, new, Some(index))?;
        graph
    };
    graph.write(doc, page)?;
    relayout(doc, page, &s, Trigger::Lanes, Anchor::List)?;
    Ok(Inserted { id: new, list: list_id, index, made: None })
}

/// Moves a lane (or a phase) to the place `to` of its list, from 0 (`swimlane.move`): it takes what it holds, and the others make room.
pub fn move_to(doc: &mut Document, page: u32, id: u32, to: usize) -> Result<usize, Problem> {
    let (s, list, order) = {
        let p = Page::read(doc, page)?;
        if !p.has(id) {
            return Err(Problem::bad(format!("page {page} has no shape {id}")));
        }
        let kind = role(&p, id)?;
        if !matches!(kind, Role::Lane | Role::Phase) {
            return Err(Problem::bad(format!("shape {id} is not a lane or a phase of a cross-functional flowchart")));
        }
        let s = structure_of(&p, id)?.ok_or_else(|| not_part(id))?;
        let list = p.list_of(id)?.ok_or_else(|| not_part(id))?;
        let mut order = p.list_members_of(list)?;
        let from = order.iter().position(|m| *m == id).ok_or_else(|| not_part(id))?;
        order.remove(from);
        let to = to.min(order.len());
        order.insert(to, id);
        (s, list, order)
    };
    let index = order.iter().position(|m| *m == id).unwrap_or(0);
    set_order(doc, page, list, &order)?;
    relayout(doc, page, &s, Trigger::Lanes, Anchor::List)?;
    Ok(index)
}

/// What `swimlane.resize` is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sizes {
    /// A lane's thickness, or a phase's width, in drawing units.
    pub size: Option<f64>,
    /// The length of the lanes, in drawing units.
    pub length: Option<f64>,
}

/// Resizes a lane's thickness, a phase's width or the length of the lanes (`swimlane.resize`), and lays the structure out again. A phase made wider takes the room from the next one; the lanes'
/// length is not changed by it. → whether anything changed.
pub fn resize(doc: &mut Document, page: u32, id: u32, sizes: Sizes) -> Result<bool, Problem> {
    let before = doc.clone();
    let (s, kind, to_write) = {
        let p = Page::read(doc, page)?;
        if !p.has(id) {
            return Err(Problem::bad(format!("page {page} has no shape {id}")));
        }
        let s = structure_for(&p, Some(id))?;
        let kind = role(&p, id)?;
        let dir = direction(&p, s.lanes)?.ok_or_else(|| not_part(s.lanes))?;
        let least = MIN_INCHES * p.k;
        let mut to_write: Option<Rect> = None;
        if let Some(size) = sizes.size {
            let size = checked("size", size)?;
            if size < least {
                return Err(least_words("`size`"));
            }
            let now = p.bounds(id)?.map(Screen::of).ok_or_else(|| Problem::failed(format!("shape {id} has no box")))?;
            let to = match (kind, dir) {
                (Role::Lane, Dir::Down) => Screen { b: now.t + size, ..now },
                (Role::Lane, Dir::Across) => Screen { r: now.l + size, ..now },
                (Role::Phase, Dir::Down) => Screen { r: now.l + size, ..now },
                (Role::Phase, Dir::Across) => Screen { b: now.t + size, ..now },
                _ => {
                    return Err(Problem::bad(format!(
                        "`size` is for a lane (its thickness) or a phase (its width), and shape {id} is neither: `length` sets the length of the lanes"
                    )));
                }
            };
            to_write = Some(to.rect());
        }
        if let Some(length) = sizes.length {
            let length = checked("length", length)?;
            if length < least {
                return Err(least_words("`length`"));
            }
        }
        (s, kind, to_write)
    };
    if let Some(rect) = to_write {
        write_box(doc, page, id, rect)?;
    }
    let trigger = match (sizes.length, kind) {
        (Some(length), _) => Trigger::Length(length),
        (None, Role::Phase) => Trigger::Phase(id),
        (None, _) => Trigger::Lanes,
    };
    relayout(doc, page, &s, trigger, Anchor::List)?;
    Ok(*doc != before)
}

/// A shape's side of the heading as `container.heading` has it.
fn heading_of(p: &Page<'_>, id: u32) -> Result<Side, Problem> {
    Ok(Settings::read(p, id)?.heading)
}

/// The side the label of a new lane or phase put beside `id` is on: the side of `id`'s own label when DiaCraft drew it, and `fresh` beside a lane or a phase of a file, whose label is drawn
/// by its own cells.
fn side_beside(p: &Page<'_>, id: u32, fresh: Side) -> Result<Side, Problem> {
    let settings = Settings::read(p, id)?;
    Ok(if settings.style.is_some() { settings.heading } else { fresh })
}

fn swapped(side: Side) -> Side {
    match side {
        Side::Left => Side::Top,
        Side::Top => Side::Left,
        Side::Right => Side::Bottom,
        Side::Bottom => Side::Right,
    }
}

/// Puts the heading of each lane (or phase) of `groups` on the side given, one command for the lanes that share a side.
fn set_sides(doc: &mut Document, page: u32, groups: Vec<(Side, Vec<u32>)>) -> Result<(), Problem> {
    for (side, ids) in groups {
        if !ids.is_empty() {
            super::set_heading(doc, page, &ids, Some(side))?;
        }
    }
    Ok(())
}

/// Groups shapes by the side each is to have.
fn by_side(wanted: Vec<(u32, Side)>) -> Vec<(Side, Vec<u32>)> {
    let mut groups: Vec<(Side, Vec<u32>)> = Vec::new();
    for (id, side) in wanted {
        match groups.iter_mut().find(|(s, _)| *s == side) {
            Some((_, ids)) => ids.push(id),
            None => groups.push((side, vec![id])),
        }
    }
    groups
}

/// Rotate Lane Label (`swimlane.rotate_label`): the label of each lane named (a frame or a list names all its lanes) moves its strip to the other of left and top, so that the text turns a
/// quarter. The text stays. → the lanes whose label turned.
pub fn rotate_label(doc: &mut Document, page: u32, ids: &[u32]) -> Result<Vec<u32>, Problem> {
    let (wanted, lanes) = {
        let p = Page::read(doc, page)?;
        let mut lanes = Unique::new();
        for id in ids {
            if !p.has(*id) {
                return Err(Problem::bad(format!("page {page} has no shape {id}")));
            }
            match role(&p, *id)? {
                Role::Lane => {
                    lanes.push(*id);
                }
                Role::Frame | Role::Lanes => {
                    let s = structure_of(&p, *id)?.ok_or_else(|| not_part(*id))?;
                    let named = p.list_members_of(s.lanes)?;
                    // Each lane's label is restyled: a list of more lanes than a list is laid out with is not turned at once.
                    if named.len() > MAX_LIST_MEMBERS {
                        return Err(Problem::failed(format!(
                            "the list {} holds {} lanes, more than the {MAX_LIST_MEMBERS} whose labels are turned at once: name fewer lanes",
                            s.lanes,
                            named.len()
                        )));
                    }
                    for lane in named {
                        lanes.push(lane);
                    }
                }
                _ => return Err(Problem::bad(format!("shape {id} is not a lane, and has no lane label to rotate"))),
            }
        }
        let lanes = lanes.into_vec();
        let mut wanted: Vec<(u32, Side)> = Vec::with_capacity(lanes.len());
        for lane in &lanes {
            let side = heading_of(&p, *lane)?;
            wanted.push((*lane, if side == Side::Left { Side::Top } else { Side::Left }));
        }
        (wanted, lanes)
    };
    set_sides(doc, page, by_side(wanted))?;
    Ok(lanes)
}

/// Swimlane orientation (`swimlane.orientation`): the structure that `id` is a part of is turned over its diagonal about its top left corner. → whether it changed.
pub fn set_orientation(doc: &mut Document, page: u32, id: u32, want: Dir) -> Result<bool, Problem> {
    struct Turn {
        parts: Vec<(u32, Rect)>,
        moves: Vec<(u32, (f64, f64))>,
        sides: Vec<(u32, Side)>,
        lists: Vec<(usize, u32, Dir)>,
    }
    let before = doc.clone();
    let turn = {
        let p = Page::read(doc, page)?;
        let s = structure_for(&p, Some(id))?;
        let dir = direction(&p, s.lanes)?.ok_or_else(|| not_part(s.lanes))?;
        if dir == want {
            return Ok(false);
        }
        let lanes = p.list_members_of(s.lanes)?;
        let phases = match s.phases {
            Some(list) => p.list_members_of(list)?,
            None => Vec::new(),
        };
        if lanes.len() > MAX_LIST_MEMBERS || phases.len() > MAX_LIST_MEMBERS {
            return Err(Problem::failed(format!("a list of more than {MAX_LIST_MEMBERS} members is not turned")));
        }
        let mut parts = Unique::new();
        for id in s.frame.iter().chain(std::iter::once(&s.lanes)).chain(s.phases.iter()).chain(&lanes).chain(&phases) {
            parts.push(*id);
        }
        let parts = parts.into_vec();
        // A part another program drew has its label, and the look of its sides, in its own cells: a turn would leave them lying across it.
        for part in &parts {
            if own_role(&p, *part)?.is_none() {
                return Err(Problem::bad(format!(
                    "shape {part} of this cross-functional flowchart was drawn by another program, and its label and sides are drawn by its own cells, which a turn would leave across it: \
                     Orientation turns the swimlanes DiaCraft made"
                )));
            }
        }
        let mut boxes: Vec<(u32, Screen)> = Vec::with_capacity(parts.len());
        for part in &parts {
            let b = p
                .bounds(*part)?
                .map(Screen::of)
                .ok_or_else(|| Problem::failed(format!("shape {part} has no box, so the structure is not turned")))?;
            if p.sheet(*part).is_some_and(|sh| p.reader.pose(&sh).is_turned_or_flipped()) {
                return Err(Problem::failed(format!("shape {part} is turned or flipped, so the structure is not turned")));
            }
            boxes.push((*part, b));
        }
        let whole = boxes
            .iter()
            .skip(1)
            .fold(boxes.first().map(|(_, b)| *b), |a, (_, b)| a.map(|a| a.union(*b)))
            .ok_or_else(|| Problem::failed("a structure with no shape"))?;
        let (pl, pt) = (whole.l, whole.t);
        // What the lanes hold, nested ones too: each shape's middle goes over the diagonal, its size stays.
        let held = p.contents(&lanes)?;
        let part_set: HashSet<u32> = parts.iter().copied().collect();
        let mut moves: Vec<(u32, (f64, f64))> = Vec::with_capacity(held.len());
        for h in held.iter().filter(|h| !part_set.contains(h)) {
            let Some(b) = p.bounds(*h)?.map(Screen::of) else { continue };
            let (cx, cy) = (b.l.midpoint(b.r), b.t.midpoint(b.b));
            let (nx, ny) = (pl + (cy - pt), pt + (cx - pl));
            moves.push((*h, (nx - cx, -(ny - cy))));
        }
        let turned_box = |b: Screen| Screen { l: pl + (b.t - pt), t: pt + (b.l - pl), r: pl + (b.b - pt), b: pt + (b.r - pl) };
        let mut sides: Vec<(u32, Side)> = Vec::new();
        for lane in lanes.iter().chain(&phases) {
            sides.push((*lane, swapped(heading_of(&p, *lane)?)));
        }
        let mut lists: Vec<(usize, u32, Dir)> = Vec::new();
        if let Some(at) = p.at(s.lanes) {
            lists.push((at, s.lanes, want));
        }
        if let (Some(list), Some(at)) = (s.phases, s.phases.and_then(|l| p.at(l))) {
            lists.push((at, list, want.turned()));
        }
        Turn { parts: boxes.iter().map(|(id, b)| (*id, turned_box(*b).rect())).collect(), moves, sides, lists }
    };
    if !turn.moves.is_empty() {
        let mut paths = Vec::with_capacity(turn.moves.len());
        for (id, d) in &turn.moves {
            paths.push((*id, require_shape(doc, page, *id)?, *d));
        }
        crate::draw::shapes::write_moves(doc, page, &paths)?;
    }
    // The outlines are made again for the boxes the parts have turned to (and the strips go with them): the sides are put by `container.heading` after the boxes are written.
    let outlines: Vec<Option<Outline>> = {
        let p = Page::read(doc, page)?;
        let mut out = Vec::with_capacity(turn.parts.len());
        for (part, _) in &turn.parts {
            out.push(outline_of(&p, *part)?);
        }
        out
    };
    let work = super::page::Work::new();
    let size = doc.page(page).map_or(0, |pg| pg.shapes.len());
    for ((part, to), outline) in turn.parts.iter().zip(outlines) {
        work.charge(16 + size / 64)?;
        crate::draw::shapes::resize_cells(doc, page, *part, to.x0, to.y0, to.width(), to.height())?;
        if let Some((at, style, side, band)) = outline {
            super::regeometry(doc, page, at, style, side, (to.width(), to.height()), band)?;
        }
    }
    for (at, _, dir) in &turn.lists {
        put_row(doc, page, *at, DIRECTION_ROW, number_cell(dir.number()))?;
    }
    set_sides(doc, page, by_side(turn.sides))?;
    let touched: Vec<u32> = turn.parts.iter().map(|(id, _)| *id).chain(turn.moves.iter().map(|(id, _)| *id)).collect();
    crate::draw::settle(doc, &before, page, &touched, &[])?;
    Ok(*doc != before)
}

/// A separator (`swimlane.insert_phase`): a phase is split in two. `target` is the phase (the last one when none), `place` says whether the new phase is the second part (`After`) or the first
/// (`Before`), and `at` is where it is split, a coordinate of the page in drawing units along the lanes (the middle when none). A structure with no phases is given a list of two.
pub fn insert_phase(
    doc: &mut Document,
    page: u32,
    target: Option<u32>,
    place: Place,
    at: Option<f64>,
    label: &str,
    above: u32,
) -> Result<Inserted, Problem> {
    require_page(doc, page)?;
    super::super::text_edit::check_text(label)?;
    let s = {
        let p = Page::read(doc, page)?;
        structure_for(&p, target)?
    };
    if s.phases.is_none() {
        return first_phases(doc, page, &s, label, above);
    }
    let (new, index, to_write, list_id, put_at, shape) = {
        let p = Page::read(doc, page)?;
        let dir = direction(&p, s.lanes)?.ok_or_else(|| not_part(s.lanes))?;
        let list_id = s.phases.ok_or_else(|| Problem::failed("the structure lost its phases"))?;
        let order = p.list_members_of(list_id)?;
        if order.len() >= MAX_NEW {
            return Err(Problem::failed(format!("a phase list holds at most {MAX_NEW} phases")));
        }
        let reference = match target.filter(|t| order.contains(t)) {
            Some(t) => t,
            None => *order.last().ok_or_else(|| Problem::bad("the list of phases has no phase to split"))?,
        };
        let index_of = order.iter().position(|m| *m == reference).unwrap_or(0);
        let now = p.bounds(reference)?.map(Screen::of).ok_or_else(|| Problem::failed(format!("phase {reference} has no box")))?;
        let least = MIN_INCHES * p.k;
        // The phase is `width` wide along the lanes, beginning at `start`.
        let (start, width) = match dir {
            Dir::Down => (now.l, now.width()),
            Dir::Across => (now.t, now.height()),
        };
        let first = match at {
            Some(a) => {
                let a = checked("at", a)?;
                let along = match dir {
                    Dir::Down => a,
                    Dir::Across => -a,
                };
                along - start
            }
            None => width / 2.0,
        };
        if first < least || width - first < least {
            return Err(Problem::bad(format!("a phase is at least {MIN_INCHES} inch wide, and splitting it there leaves a part thinner than that")));
        }
        // `Before` gives the first part to the new phase and the second to the one split; `After` the other way round.
        let (reference_part, new_part) = match place {
            Place::After => ((start, start + first), (start + first, start + width)),
            Place::Before => ((start + first, start + width), (start, start + first)),
        };
        let boxed = |(a, b): (f64, f64)| match dir {
            Dir::Down => Screen { l: a, r: b, ..now },
            Dir::Across => Screen { t: a, b, ..now },
        };
        let side = side_beside(&p, reference, phase_side(dir))?;
        let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
        if count >= MAX_SHAPES_PER_PAGE {
            return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
        }
        let new = highest.max(above).checked_add(1).ok_or_else(|| Problem::failed("no shape ids are left on the page"))?;
        let shape = phase_shape(&part(new, boxed(new_part), side, BAND_INCHES * p.k, 0.0, label));
        let index = match place {
            Place::After => index_of + 1,
            Place::Before => index_of,
        };
        (new, index, (reference, boxed(reference_part).rect()), list_id, after_all(&p, &order, list_id), shape)
    };
    insert_at(doc, page, put_at, shape)?;
    let (reference, rect) = to_write;
    write_box(doc, page, reference, rect)?;
    let graph = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        graph.list_put(&p, list_id, new, Some(index))?;
        graph
    };
    graph.write(doc, page)?;
    relayout(doc, page, &s, Trigger::Lanes, Anchor::List)?;
    Ok(Inserted { id: new, list: list_id, index, made: None })
}

/// The first separator of a structure with no phases: a list of two phases that share the length.
fn first_phases(doc: &mut Document, page: u32, s: &Structure, label: &str, above: u32) -> Result<Inserted, Problem> {
    let frame = s.frame.ok_or_else(|| Problem::bad("the lanes have no frame to hold a list of phases"))?;
    let (dir, lanes_box, k, highest, count, frame_at) = {
        let p = Page::read(doc, page)?;
        let dir = direction(&p, s.lanes)?.ok_or_else(|| not_part(s.lanes))?;
        let lanes_box = p.bounds(s.lanes)?.map(Screen::of).ok_or_else(|| Problem::failed("the list of lanes has no box"))?;
        let (highest, count) = page_stats(doc, page).ok_or_else(|| Problem::bad(format!("there is no page {page}")))?;
        (dir, lanes_box, p.k, highest, count, p.at(frame).unwrap_or(0))
    };
    if count.saturating_add(3) > MAX_SHAPES_PER_PAGE {
        return Err(Problem::failed(format!("a page holds at most {MAX_SHAPES_PER_PAGE} shapes")));
    }
    let base = highest.max(above);
    let ids: Vec<u32> =
        (1..=3_u32).map(|n| base.checked_add(n).ok_or_else(|| Problem::failed("no shape ids are left on the page"))).collect::<Result<_, _>>()?;
    let (list_id, first, second) = (ids[0], ids[1], ids[2]);
    let band = PHASE_BAND_INCHES * k;
    let strip = BAND_INCHES * k;
    let whole = lanes_box.cross(dir);
    let (boxes, band_box) = band_boxes(dir, lanes_box, band, &[whole / 2.0, whole / 2.0]);
    let mut list_ties = Ties::default();
    list_ties.add(IN_CONTAINER, frame);
    list_ties.add(LIST_MEMBERS, first);
    list_ties.add(LIST_MEMBERS, second);
    let mut list = list_shape(
        &part(list_id, band_box.unwrap_or(lanes_box), Side::Top, BAND_INCHES * k, 0.0, ""),
        crate::stencil::swimlane::PHASES,
        dir.turned().number(),
    );
    set_ties(&mut list, &list_ties);
    let mut made = vec![list];
    for (id, b) in [first, second].iter().zip(&boxes) {
        let mut ties = Ties::default();
        ties.add(IN_LIST, list_id);
        let mut phase = phase_shape(&part(*id, *b, phase_side(dir), strip, 0.0, label));
        set_ties(&mut phase, &ties);
        made.push(phase);
    }
    for (n, shape) in made.into_iter().enumerate() {
        insert_at(doc, page, frame_at + 1 + n, shape)?;
    }
    let graph = {
        let p = Page::read(doc, page)?;
        let mut graph = Graph::new();
        graph.join(&p, frame, list_id, None)?;
        graph
    };
    graph.write(doc, page)?;
    let again = {
        let p = Page::read(doc, page)?;
        structure_of(&p, list_id)?.ok_or_else(|| Problem::failed("the list of phases was not found after it was made"))?
    };
    relayout(doc, page, &again, Trigger::Lanes, Anchor::List)?;
    Ok(Inserted {
        id: first,
        list: list_id,
        index: 0,
        made: Some(Created { frame, lane_list: s.lanes, lanes: Vec::new(), phase_list: Some(list_id), phases: vec![first, second] }),
    })
}

/// One lane of a structure, as a command reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct PartInfo {
    pub id: u32,
    /// A lane's thickness or a phase's width, in paper inches.
    pub size: f64,
    pub heading: Side,
    pub text: String,
    /// The box on the page in paper inches: left, bottom, right, top.
    pub rect: Option<[f64; 4]>,
}

/// A structure, as `swimlane.info` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureInfo {
    pub frame: Option<u32>,
    pub lane_list: u32,
    pub phase_list: Option<u32>,
    pub dir: Dir,
    /// The length of the lanes, in paper inches.
    pub length: f64,
    /// The frame's box (or the list of lanes', when there is no frame) in paper inches: left, bottom, right, top.
    pub rect: Option<[f64; 4]>,
    pub lanes: Vec<PartInfo>,
    pub phases: Vec<PartInfo>,
}

/// A box in paper inches: left, bottom, right, top.
fn inches(p: &Page<'_>, r: Rect) -> [f64; 4] {
    let k = if p.k > 0.0 { p.k } else { 1.0 };
    [r.x0 / k, r.y0 / k, r.x1 / k, r.y1 / k]
}

/// The most characters of a label that `swimlane.info` tells.
const MAX_LABEL_CHARS: usize = 256;

/// The most parts of a label's text that are walked: a part that is no characters (a field, a marker, a line of formatting) is walked for a step like any other, and a text of a million of
/// them is read as far as this and no further.
const MAX_LABEL_PARTS: usize = 1_024;

/// The first [`MAX_LABEL_CHARS`] characters of the shape's text, read from the shape itself (a search of the page for each of many lanes would cost their product, and a text of a hundred
/// thousand characters is not copied for each). **A step of the command's budget for the label and for each part of its text that is walked**, whatever the part is, charged before the part
/// is looked at; the walk ends at [`MAX_LABEL_PARTS`] parts, or when the characters are enough, before the next part of any kind is visited.
pub(super) fn label_of(p: &Page<'_>, id: u32) -> Result<String, Problem> {
    p.work.charge(4)?;
    let Some(at) = p.shape_ref(id) else { return Ok(String::new()) };
    let Some(shape) = p.doc.shape(&at) else { return Ok(String::new()) };
    let mut out = String::new();
    let mut left = MAX_LABEL_CHARS;
    for part in shape.text.iter().flat_map(|t| t.parts.iter()).take(MAX_LABEL_PARTS) {
        if left == 0 {
            break;
        }
        p.work.charge(1)?;
        if let diacraft_doc::TextPart::Chars(chars) = part {
            let before = out.len();
            out.extend(chars.chars().take(left));
            left = left.saturating_sub(out[before..].chars().count());
        }
    }
    Ok(out)
}

/// What `swimlane.info` tells of each of the lanes or phases `ids`, which lie along `along`; `fresh` is the side of the label of one another program drew (its own cells draw it).
fn info_of(p: &Page<'_>, ids: &[u32], along: Dir, fresh: Side) -> Result<Vec<PartInfo>, Problem> {
    let mut out = Vec::with_capacity(ids.len().min(MAX_LIST_MEMBERS));
    for id in ids.iter().take(MAX_LIST_MEMBERS) {
        let bounds = p.bounds(*id)?;
        let size = bounds.map_or(0.0, |r| Screen::of(r).main(along));
        let text = label_of(p, *id)?;
        out.push(PartInfo {
            id: *id,
            size: if p.k > 0.0 { size / p.k } else { size },
            heading: side_beside(p, *id, fresh)?,
            text,
            rect: bounds.map(|r| inches(p, r)),
        });
    }
    Ok(out)
}

/// The structures of a page, each once (a query).
pub fn describe(doc: &Document, page: u32) -> Result<Vec<StructureInfo>, Problem> {
    let p = Page::read(doc, page)?;
    let mut out: Vec<StructureInfo> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    for id in &p.containers {
        if role(&p, *id)? != Role::Lanes || !seen.insert(*id) {
            continue;
        }
        let Some(dir) = direction(&p, *id)? else { continue };
        let Some(s) = structure_of(&p, *id)? else { continue };
        let lanes = p.list_members_of(s.lanes)?;
        let phases = match s.phases {
            Some(list) => p.list_members_of(list)?,
            None => Vec::new(),
        };
        let length = match p.bounds(s.lanes)?.map(Screen::of) {
            Some(b) if p.k > 0.0 => b.cross(dir) / p.k,
            Some(b) => b.cross(dir),
            None => 0.0,
        };
        let whole = match s.frame {
            Some(frame) => p.bounds(frame)?,
            None => p.bounds(s.lanes)?,
        };
        out.push(StructureInfo {
            frame: s.frame,
            lane_list: s.lanes,
            phase_list: s.phases,
            dir,
            length,
            rect: whole.map(|r| inches(&p, r)),
            lanes: info_of(&p, &lanes, dir, lane_side(dir))?,
            phases: info_of(&p, &phases, dir.turned(), phase_side(dir))?,
        });
    }
    Ok(out)
}

/// The structure that each of the shapes `ids` is a part of, as (the shape, the role word, the lane list) for the shapes that are parts: what the window asks, to know what is selected.
pub fn roles_of(doc: &Document, page: u32, ids: &[u32]) -> Result<Vec<(u32, &'static str, u32)>, Problem> {
    let p = Page::read(doc, page)?;
    let mut out = Vec::new();
    for id in ids {
        let word = match role(&p, *id)? {
            Role::Frame => "frame",
            Role::Lanes => "lanes",
            Role::Phases => "phases",
            Role::Lane => "lane",
            Role::Phase => "phase",
            Role::Other => continue,
        };
        if let Some(s) = structure_of(&p, *id)? {
            out.push((*id, word, s.lanes));
        }
    }
    Ok(out)
}

/// The labels' placeholders, for the commands' words.
pub const PLACEHOLDERS: (&str, &str) = (LANE_LABEL, PHASE_LABEL);
