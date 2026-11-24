//! A container's size: the box that holds its members, Fit to Contents, the automatic resizing, and the shapes on its edges.
//!
//! (ours where the specifications are silent: "Fit to Contents resizes the container to just enclose its shapes", Automatic Resize
//! None, Expand as Needed or Always Fit to Contents). **The box a container needs** is the box that holds its members' boxes, grown by its margin on every side and by the
//! heading's strip on the heading's side (the strip is part of the container, so a member never lies under it). Fit to Contents makes the container exactly that box; Always Fit
//! to Contents does it after every change of a member; Expand as Needed makes the container the box that holds both what it is and what it needs, so it grows and never
//! shrinks; None leaves its size alone. A container with no member is not resized (there is nothing to hold). A container that is turned or flipped, or is a group, is not
//! resized (the box of its members is not square to it): the command says so. A container that holds itself through its members (a cycle in a file) is left alone. A container on
//! a locked layer is not resized either, and the command notes it; the ones above it are fitted round it as it is.
//!
//! **One plan, one write.** A container's own change of size changes the box the container above it needs, so the fit goes up the nesting. It is **planned in memory, in one
//! pass**: the containers a fit reaches (the ones named and every container above them) are put in order, each after the containers it holds, and each one's new box is worked out
//! from its members' boxes with the boxes already planned for the containers among them. Nothing is read twice and nothing is written until the plan is whole; then every container
//! is sized in one write (`draw::shapes::resize_cells`) and the page is recalculated once. **A command fits at most [`MAX_FITTED`] containers**: a plan that reaches more (a
//! thousand containers that hold the same shapes) is refused in words before anything is written, and a thousand levels of nesting are a thousand containers, not a thousand
//! passes (the security scan's third finding). The work is the edit's one budget (`page::Work`), and each container sized is charged in proportion to the page.

use std::collections::{HashMap, HashSet};

use diacraft_doc::Document;

use super::page::{Page, Rect, Unique, Work};
use super::settings::{Resize, Settings};
use crate::draw::shapes::{move_each, resize_cells};
use crate::draw::ties::ON_EDGE;
use crate::draw::{Problem, settle};
use crate::stencil::container::{Side, StyleSpec, find_style};

/// The most containers one command fits (the ones it is asked to fit and every one above them). A hundred levels of nesting is a drawing; thousands of containers that hold the same
/// shapes are not, and the plan is refused before anything is written.
pub const MAX_FITTED: usize = 4_096;

/// Shapes to move and the distance each goes by, in drawing units.
pub(super) type Moves = Vec<(u32, (f64, f64))>;

/// The smallest a container is made, in drawing units: a box of no width or height is not a box.
const LEAST: f64 = 1.0e-3;

/// What a fit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fitted {
    /// The containers whose size changed.
    pub resized: Vec<u32>,
    /// The containers that were not resized because they are turned or flipped, a group, or hold themselves.
    pub skipped: Vec<u32>,
    /// The shapes moved to stay on the edge of a container that was resized.
    pub moved: Vec<u32>,
    /// The containers on a locked layer that were left as they were: not resized, and no member joined or left them. What the command's
    /// answer notes ([`super::locked_notes`]).
    pub locked: Vec<u32>,
}

impl Fitted {
    /// Adds the containers `more` to those passed over, each once.
    pub(crate) fn passed(mut self, more: Vec<u32>) -> Fitted {
        let mut seen: HashSet<u32> = self.locked.iter().copied().collect();
        for id in more {
            if seen.insert(id) {
                self.locked.push(id);
            }
        }
        self
    }
}

/// The box a container needs to hold its members, `None` when it holds none that has a box. `planned` holds the boxes already planned for containers among the members.
pub(super) fn needed(page: &Page<'_>, id: u32, settings: &Settings, planned: &HashMap<u32, Rect>) -> Result<Option<Rect>, Problem> {
    let mut all: Option<Rect> = None;
    for m in page.members_of(id)? {
        // A member that sits on an edge is on the border: it follows the container's size and does not ask for any (or each fit would grow the box to hold it, and move it out).
        if page.attached(m, id)? {
            continue;
        }
        let b = match planned.get(&m) {
            Some(b) => Some(*b),
            None => page.bounds(m)?,
        };
        if let Some(b) = b {
            all = Some(all.map_or(b, |a| a.union(b)));
        }
    }
    let Some(r) = all else { return Ok(None) };
    let m = settings.margin;
    let mut need = Rect { x0: r.x0 - m, y0: r.y0 - m, x1: r.x1 + m, y1: r.y1 + m };
    match settings.heading {
        Side::Top => need.y1 += settings.band,
        Side::Bottom => need.y0 -= settings.band,
        Side::Left => need.x0 -= settings.band,
        Side::Right => need.x1 += settings.band,
    }
    Ok(Some(need))
}

/// What one container becomes: its new box and the shapes to move to keep the ones on its edges there.
struct Plan {
    id: u32,
    to: Rect,
    moves: Moves,
    /// Where the container is in the page's list and its style, for the outline to be made again for the new size, when it is one of the gallery's.
    outline: Option<(usize, &'static StyleSpec, Side, f64)>,
}

/// The containers a fit reaches, each after the containers it holds: `ids` and every container above them, in the order a plan can take them (a container's members that are
/// among them come first). A container that is in a cycle is returned in `cyclic`. Refused in words past [`MAX_FITTED`].
fn reach(page: &Page<'_>, ids: &[u32]) -> Result<(Vec<u32>, HashSet<u32>), Problem> {
    let too_many = || {
        Problem::failed(format!(
            "one command fits at most {MAX_FITTED} containers, and this one reaches more (every container above the ones named is fitted too): fit fewer, or take shapes out of some of the containers"
        ))
    };
    let mut group = Unique::new();
    let mut stack: Vec<u32> = Vec::new();
    for id in ids {
        if page.has(*id) && page.is_container(*id) && group.push(*id) {
            stack.push(*id);
        }
    }
    while let Some(c) = stack.pop() {
        for above in page.containers_of(c)? {
            if page.is_container(above) && group.push(above) {
                if group.len() > MAX_FITTED {
                    return Err(too_many());
                }
                stack.push(above);
            }
        }
        if group.len() > MAX_FITTED {
            return Err(too_many());
        }
    }
    let all: Vec<u32> = group.into_vec();
    let in_group: HashSet<u32> = all.iter().copied().collect();
    // Depth first, a container after the members of it that are in the group; 0 not seen, 1 on the way down, 2 done.
    let mut state: HashMap<u32, u8> = HashMap::with_capacity(all.len());
    let mut order: Vec<u32> = Vec::with_capacity(all.len());
    let mut cyclic: HashSet<u32> = HashSet::new();
    for start in &all {
        if state.contains_key(start) {
            continue;
        }
        let mut frames: Vec<(u32, Vec<u32>, usize)> = Vec::new();
        state.insert(*start, 1);
        frames.push((*start, page.members_of(*start)?.into_iter().filter(|m| in_group.contains(m)).collect(), 0));
        while let Some(frame) = frames.last_mut() {
            let (c, children, at) = (frame.0, &frame.1, frame.2);
            if let Some(child) = children.get(at).copied() {
                frame.2 += 1;
                match state.get(&child).copied() {
                    None => {
                        state.insert(child, 1);
                        let below: Vec<u32> = page.members_of(child)?.into_iter().filter(|m| in_group.contains(m)).collect();
                        frames.push((child, below, 0));
                    }
                    Some(1) => {
                        cyclic.insert(child);
                        cyclic.insert(c);
                    }
                    Some(_) => {}
                }
            } else {
                state.insert(c, 2);
                order.push(c);
                frames.pop();
            }
        }
    }
    Ok((order, cyclic))
}

/// Plans the new box of a container: `forced` fits it whatever its setting says (the Fit to Contents command).
fn plan_one(page: &Page<'_>, id: u32, forced: bool, planned: &HashMap<u32, Rect>, skipped: &mut Vec<u32>) -> Result<Option<Plan>, Problem> {
    let settings = Settings::read(page, id)?;
    let mode = if forced { Resize::Fit } else { settings.resize };
    if mode == Resize::None || !page.usable(id)? {
        return Ok(None);
    }
    let Some(sheet) = page.sheet(id) else { return Ok(None) };
    let is_group = page.shape_ref(id).and_then(|r| page.doc.shape(&r)).is_some_and(|s| !s.children.is_empty());
    if page.reader.pose(&sheet).is_turned_or_flipped() || is_group {
        skipped.push(id);
        return Ok(None);
    }
    let (Some(now), Some(need)) = (page.bounds(id)?, needed(page, id, &settings, planned)?) else { return Ok(None) };
    let to = match mode {
        Resize::Fit => need,
        _ => {
            if now.holds(&need) {
                return Ok(None);
            }
            now.union(need)
        }
    };
    if now.holds(&to) && to.holds(&now) {
        return Ok(None);
    }
    // A container on a locked layer is not resized: passed over, and noted.
    if page.on_locked_layer(id)? {
        page.pass_over(id);
        return Ok(None);
    }
    let to = Rect { x1: to.x1.max(to.x0 + LEAST), y1: to.y1.max(to.y0 + LEAST), ..to };
    let moves = edge_moves(page, id, &to)?;
    let outline = settings.style.as_deref().and_then(find_style).zip(page.at(id)).map(|(style, at)| (at, style, settings.heading, settings.band));
    Ok(Some(Plan { id, to, moves, outline }))
}

/// The moves that put the shapes on the edges of container `id` back on them when the container is the box `to`: each pin goes to the edge it is on.
pub(super) fn edge_moves(page: &Page<'_>, id: u32, to: &Rect) -> Result<Moves, Problem> {
    let mut moves = Vec::new();
    for member in page.members_of(id)? {
        let ties = page.ties(member)?;
        let Some(edge) = ON_EDGE.iter().position(|code| ties.contains(*code, id)) else { continue };
        let Some(b) = page.bounds(member)? else { continue };
        let (cx, cy) = b.centre();
        let (dx, dy) = match edge {
            0 => (to.x0 - cx, 0.0),
            1 => (to.x1 - cx, 0.0),
            2 => (0.0, to.y1 - cy),
            _ => (0.0, to.y0 - cy),
        };
        // Along the edge it keeps its place, within the box.
        let (dx, dy) = if edge < 2 { (dx, (cy.clamp(to.y0, to.y1) - cy) + dy) } else { ((cx.clamp(to.x0, to.x1) - cx) + dx, dy) };
        if dx != 0.0 || dy != 0.0 {
            moves.push((member, (dx, dy)));
        }
    }
    Ok(moves)
}

/// Fits the containers `ids` and, in turn, the containers above each one that changes size: planned in one pass, written in one. `forced` is the Fit to Contents
/// command; without it each container follows its own Automatic Resize setting.
pub(super) fn refit(doc: &mut Document, page: u32, ids: &[u32], forced: bool) -> Result<Fitted, Problem> {
    let (plans, mut skipped, locked) = {
        let p = Page::read(doc, page)?;
        let (order, cyclic) = reach(&p, ids)?;
        let named: HashSet<u32> = ids.iter().copied().collect();
        let mut planned: HashMap<u32, Rect> = HashMap::new();
        let mut plans: Vec<Plan> = Vec::new();
        let mut skipped: Vec<u32> = Vec::new();
        for id in order {
            if cyclic.contains(&id) {
                skipped.push(id);
                continue;
            }
            if let Some(plan) = plan_one(&p, id, forced && named.contains(&id), &planned, &mut skipped)? {
                planned.insert(id, plan.to);
                plans.push(plan);
            }
        }
        (plans, skipped, p.passed_over())
    };
    skipped.sort_unstable();
    skipped.dedup();
    let mut out = Fitted { skipped, locked, ..Fitted::default() };
    if plans.is_empty() {
        return Ok(out);
    }
    // Every container is sized in one write and the page is recalculated once; each is charged in proportion to the page (the recalculation compares it with what it was).
    let work = Work::new();
    let size = doc.page(page).map_or(0, |p| p.shapes.len());
    work.charge(size / 16 + 1)?;
    let before = doc.clone();
    for plan in &plans {
        work.charge(16 + size / 64)?;
        resize_cells(doc, page, plan.id, plan.to.x0, plan.to.y0, plan.to.width(), plan.to.height())?;
        if let Some((at, style, side, band)) = plan.outline {
            super::regeometry(doc, page, at, style, side, (plan.to.width(), plan.to.height()), band)?;
        }
        out.resized.push(plan.id);
    }
    settle(doc, &before, page, &out.resized, &[])?;
    let moves: Vec<(u32, (f64, f64))> = plans.iter().flat_map(|p| p.moves.iter().copied()).collect();
    if !moves.is_empty() {
        work.charge(16 + size / 64)?;
        move_each(doc, page, &moves)?;
        out.moved = moves.iter().map(|(id, _)| *id).collect();
    }
    // A lane that was fitted makes the lanes after it make room.
    super::lists::after_resize(doc, page, &out.resized)?;
    Ok(out)
}
